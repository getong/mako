use anyhow::Result;
use cli_shared::model::PackageLock;
use flate2::read::GzDecoder;
use futures::future::join_all;
use serde_json;
use std::io::Read;
use tar::Archive;

use super::opfs_fs as fs;

/// Download all tgz packages to OPFS
pub async fn install_deps(lock_content: &str, _pkg: &str) -> Result<Vec<String>> {
    // Parse lock_content as PackageLock
    let lock = PackageLock::from_json(lock_content)?;
    let project_name = lock.name.clone();

    // Write package.json to root
    if let Some(root_pkg) = lock.packages.get("") {
        let pkg_json = serde_json::to_string_pretty(root_pkg).unwrap_or("{}".to_string());
        fs::create_dir_all(&format!("{}/node_modules", &project_name)).await?;
        fs::write(&format!("{}/package.json", &project_name), &pkg_json).await?;
    }

    // Prepare tasks for parallel execution
    let mut tasks = Vec::new();
    for (path, pkg) in lock.packages.iter() {
        if path.is_empty() {
            continue;
        }
        let name = pkg.get_name(path);
        let version = pkg.get_version();
        let tgz_url = pkg.resolved.clone();
        let project_name = project_name.clone();
        let name2 = name.clone();
        let version2 = version.clone();

        // Each task is an async block for parallel execution
        let task = async move {
            if let Some(tgz_url) = tgz_url {
                let url_path = tgz_url.split('/').collect::<Vec<_>>();
                let tgz_file_name = url_path.last().unwrap_or(&"package.tgz");
                let tgz_store_path = format!("/stores/{name}/-/{tgz_file_name}");
                let unpacked_dir = format!("/stores/{name}/-/{tgz_file_name}-unpack");
                let unpack_dir = format!("{project_name}/node_modules/{name}");

                // If unpacked_dir exists, just create fuse link to node_modules
                if fs::exists(&unpacked_dir).await.unwrap_or(false) {
                    match fuse_link(&unpacked_dir, &unpack_dir).await {
                        Ok(_) => format!("{name}@{version}: fuse link from unpacked cache"),
                        Err(e) => format!("{name}@{version}: fuse link error: {e:?}"),
                    }
                } else {
                    // If tgz exists, use it, else download
                    let tgz_bytes = if exists(&tgz_store_path).await.unwrap_or(false) {
                        match read_bytes(&tgz_store_path).await {
                            Ok(bytes) => bytes,
                            Err(e) => return format!("{name}@{version}: read cache error: {e:?}"),
                        }
                    } else {
                        match download_bytes(&tgz_url).await {
                            Ok(bytes) => {
                                if let Err(e) = write_bytes(&tgz_store_path, &bytes).await {
                                    return format!("{name}@{version}: write tgz error: {e:?}");
                                }
                                bytes
                            }
                            Err(e) => return format!("{name}@{version}: download error: {e:?}"),
                        }
                    };

                    // First extract to node_modules
                    match extract_tgz_bytes(&tgz_bytes, &unpacked_dir).await {
                        Ok(_) => {
                            // Then create fuse link from node_modules to unpacked_dir for cache
                            match fuse_link(&unpacked_dir, &unpack_dir).await {
                                Ok(_) => format!(
                                    "{name2}@{version2}: extracted to node_modules and fuse linked to unpacked"
                                ),
                                Err(e) => format!(
                                    "{name2}@{version2}: extracted to node_modules but fuse link error: {e:?}"
                                ),
                            }
                        }
                        Err(e) => format!("{name2}@{version2}: extract error: {e:?}"),
                    }
                }
            } else {
                format!("{name}@{version}: no resolved field")
            }
        };
        tasks.push(task);
    }

    // Run all tasks in parallel and collect results
    let results = join_all(tasks).await;
    Ok(results)
}

/// Extract tgz bytes to directory
pub async fn extract_tgz_bytes(tgz_bytes: &[u8], extract_dir: &str) -> Result<()> {
    let gz = GzDecoder::new(tgz_bytes);
    let mut archive = Archive::new(gz);
    let entries = archive.entries()?;

    for entry in entries {
        let mut entry = entry?;
        let path = entry.path()?;
        let path_str = path.to_string_lossy().to_string();

        // Remove the first-level "package" directory if present
        let out_path = if let Some(stripped) = path_str.strip_prefix("package/") {
            format!("{extract_dir}/{stripped}")
        } else if path_str == "package" {
            // Skip the root package directory
            continue;
        } else {
            format!("{extract_dir}/{path_str}")
        };

        if entry.header().entry_type().is_file() {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            // Write the file to the output path
            write_bytes(&out_path, &contents).await?;
        }
    }
    Ok(())
}

/// Check if file exists
async fn exists(path: &str) -> Result<bool> {
    fs::exists(path).await
}

/// Read file as bytes
async fn read_bytes(path: &str) -> Result<Vec<u8>> {
    fs::read_bytes(path).await
}

/// Write bytes to file
async fn write_bytes(path: &str, bytes: &[u8]) -> Result<()> {
    // create parent dir if not exists
    let parent_dir = if let Some(last_slash) = path.rfind('/') {
        &path[..last_slash]
    } else {
        ""
    };
    if !parent_dir.is_empty() {
        fs::create_dir_all(parent_dir).await?;
    }
    fs::write_bytes(path, bytes).await
}

/// Download bytes from URL
async fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

/// Create fuse link (placeholder implementation)
async fn fuse_link(src: &str, dst: &str) -> Result<()> {
    // Create the destination directory if it doesn't exist
    fs::create_dir_all(dst).await?;

    let link_file_path = format!("{dst}/fuse.link");

    // Check if fuse.link already exists
    if fs::exists(&link_file_path).await? {
        // Read existing content
        let existing_content = fs::read(&link_file_path).await?;
        let mut links: Vec<String> = existing_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|s| s.to_string())
            .collect();

        // Add new link if not already present
        if !links.contains(&src.to_string()) {
            links.push(src.to_string());
        }

        // Write back all links
        let new_content = links.join("\n") + "\n";
        fs::write(&link_file_path, &new_content).await?;
    } else {
        // Create new fuse.link file with the source path
        let link_content = format!("{src}\n");
        fs::write(&link_file_path, &link_content).await?;
    }

    Ok(())
}
