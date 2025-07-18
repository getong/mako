use anyhow::Result;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::OnceLock;
use web_sys::console;

// Global CWD static variable accessible to all modules
static CWD: OnceLock<Mutex<String>> = OnceLock::new();

/// Directory entry with name and type information
#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_dir: bool,
}

pub mod opfs_fs {
    use super::*;

    async fn prepare_path(path: &str) -> Result<String> {
        if path.starts_with('/') {
            Ok(path.to_string())
        } else {
            let cwd = cwd::get_cwd().await?;
            console::log_1(&format!("CWD: {cwd}").into());
            Ok(format!("{cwd}/{path}"))
        }
    }

    /// Get package name from a path that contains node_modules
    fn get_package_name(path: &str) -> Option<String> {
        // Find node_modules in the path
        if let Some(node_modules_pos) = path.find("node_modules") {
            // Get the part after node_modules/
            let after_node_modules = &path[node_modules_pos + "node_modules".len()..];

            // Remove leading slash if present
            let after_node_modules = after_node_modules.trim_start_matches('/');

            // Split by '/' to get components
            let components: Vec<&str> = after_node_modules.split('/').collect();

            if components.is_empty() {
                return None;
            }

            // Check if first component starts with @ (scoped package)
            let package_name = if components[0].starts_with('@') {
                // For scoped packages, we need two components: @scope/package
                if components.len() >= 2 {
                    format!("{}/{}", components[0], components[1])
                } else {
                    return None;
                }
            } else {
                // For regular packages, just use the first component
                components[0].to_string()
            };

            if !package_name.is_empty() {
                return Some(package_name);
            }
        }
        None
    }

    /// Get fuse.link path for a given path that contains node_modules
    fn get_fuse_link_path(path: &str) -> Option<String> {
        if let Some(package_name) = get_package_name(path) {
            console::log_1(&format!("get_fuse_link_path: {package_name}").into());
            // Find node_modules in the path
            if let Some(node_modules_pos) = path.find("node_modules") {
                // Construct the path: original_path_up_to_node_modules/node_modules/package_name/fuse.link
                let before_node_modules = &path[..node_modules_pos];
                return Some(format!(
                    "{before_node_modules}/node_modules/{package_name}/fuse.link"
                ));
            }
        }
        None
    }

    /// Read file content as string
    pub async fn read(path: &str) -> Result<String> {
        let prepared_path = prepare_path(path).await?;
        console::log_1(&format!("opfs-project read: {prepared_path}").into());

        // Check if path contains node_modules
        if prepared_path.contains("node_modules") {
            // Get the fuse.link path for this package
            if let Some(fuse_link_path) = get_fuse_link_path(&prepared_path) {
                console::log_1(&format!("Checking fuse.link at: {fuse_link_path}").into());

                // Check if fuse.link exists
                if let Ok(link_content) = tokio_fs_ext::read_to_string(&fuse_link_path).await {
                    let target_dir = link_content.lines().next().unwrap_or("").trim();
                    console::log_1(
                        &format!("Found fuse.link with target_dir: {target_dir}").into(),
                    );

                    if !target_dir.is_empty() {
                        // Get the relative path after the package name
                        if let Some(package_name) = get_package_name(&prepared_path)
                            && let Some(node_modules_pos) = prepared_path.find("node_modules")
                        {
                            // Get the part after node_modules/package_name/
                            let after_package =
                                &prepared_path[node_modules_pos + "node_modules".len()..];
                            let after_package = after_package.trim_start_matches('/');

                            // Remove the package name from the path
                            let relative_path = if after_package.starts_with(&package_name) {
                                after_package[package_name.len()..]
                                    .trim_start_matches('/')
                                    .to_string()
                            } else {
                                // Fallback: just get the filename
                                if let Some(file_name) =
                                    std::path::Path::new(&prepared_path).file_name()
                                {
                                    file_name.to_string_lossy().to_string()
                                } else {
                                    return Err(anyhow::anyhow!("Could not extract relative path"));
                                }
                            };

                            let target_path = format!("{target_dir}/{relative_path}");
                            console::log_1(
                                &format!("Trying to read from target path: {target_path}").into(),
                            );

                            // Try to read from the target directory
                            match tokio_fs_ext::read_to_string(&target_path).await {
                                Ok(content) => return Ok(content),
                                Err(e) => {
                                    console::log_1(
                                        &format!("Failed to read from target path: {e}").into(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // If no node_modules or fuse.link logic didn't work, try direct read
        let content = tokio_fs_ext::read_to_string(&prepared_path).await?;
        Ok(content)
    }

    /// Read file content as bytes
    pub async fn read_bytes(path: &str) -> Result<Vec<u8>> {
        let prepared_path = prepare_path(path).await?;
        let content = tokio_fs_ext::read(&prepared_path).await?;
        Ok(content)
    }

    /// Write content to file
    pub async fn write(path: &str, content: &str) -> Result<()> {
        // to buffer
        let buffer = content.as_bytes();
        tokio_fs_ext::write(path, buffer)
            .await
            .map_err(|e| anyhow::anyhow!("write error: {e}"))?;
        Ok(())
    }

    /// Write binary content to file
    pub async fn write_bytes(path: &str, content: &[u8]) -> Result<()> {
        tokio_fs_ext::write(path, content)
            .await
            .map_err(|e| anyhow::anyhow!("write_bytes error: {e}"))?;
        Ok(())
    }

    pub async fn create_dir_all(path: &str) -> Result<()> {
        tokio_fs_ext::create_dir_all(path)
            .await
            .map_err(|e| anyhow::anyhow!("create_dir_all error: {e}"))?;
        Ok(())
    }

    /// Remove a file
    pub async fn remove(path: &str) -> Result<()> {
        tokio_fs_ext::remove_file(path).await?;
        Ok(())
    }

    /// Read directory contents with file type information
    pub async fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
        let prepared_path = prepare_path(path).await?;
        console::log_1(&format!("opfs-project read_dir: {prepared_path}").into());

        // If no node_modules or fuse.link logic didn't work, try direct read
        let mut entries = Vec::new();

        // Check if path contains node_modules
        if prepared_path.contains("node_modules") {
            // Get the fuse.link path for this package
            if let Some(fuse_link_path) = get_fuse_link_path(&prepared_path) {
                console::log_1(&format!("Checking fuse.link at: {fuse_link_path}").into());

                // Check if fuse.link exists
                if let Ok(link_content) = tokio_fs_ext::read_to_string(&fuse_link_path).await {
                    let target_dir = link_content.lines().next().unwrap_or("").trim();
                    console::log_1(
                        &format!("Found fuse.link with target_dir: {target_dir}").into(),
                    );

                    if !target_dir.is_empty() {
                        // Get the package name and the directory name from the original path
                        if let Some(package_name) = get_package_name(&prepared_path)
                            && let Some(dir_name) = std::path::Path::new(&prepared_path).file_name()
                        {
                            let dir_name_str = dir_name.to_string_lossy();

                            // Check if the directory name matches the package name
                            if dir_name_str == package_name {
                                // Case 1: directory name matches package name (e.g., node_modules/lodash)
                                console::log_1(&format!("Directory name matches package name, reading from target_dir: {target_dir}").into());
                                return Box::pin(self::read_dir(target_dir)).await;
                            } else {
                                // Case 2: directory name doesn't match package name (e.g., node_modules/@lodash/has)
                                // Replace the package path in the target directory
                                let target_path = format!("{target_dir}/{dir_name_str}");
                                console::log_1(&format!("Directory name doesn't match package name, reading from target_path: {target_path}").into());
                                return Box::pin(self::read_dir(&target_path)).await;
                            }
                        }
                    }
                }
            }
        }

        let mut read_dir = match tokio_fs_ext::read_dir(&prepared_path).await {
            Ok(read_dir) => read_dir,
            Err(e) => {
                console::log_1(&format!("Error reading directory: {e}").into());
                return Err(anyhow::anyhow!("Error reading directory: {}", e));
            }
        };

        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();

            if let Some(name) = entry_path.file_name()
                && let Some(name_str) = name.to_str()
            {
                let meta = tokio_fs_ext::metadata(&entry_path).await?;
                let is_file = format!("{meta:?}").contains("File");
                let is_dir = format!("{meta:?}").contains("Directory");

                let dir_entry: DirEntry = DirEntry {
                    name: name_str.to_string(),
                    is_file,
                    is_dir,
                };
                entries.push(dir_entry);
            }
        }

        // if entries only contains one entry, and it is fuse.link
        // return the target dir directory
        if entries.len() == 1 && entries[0].name == "fuse.link" {
            let link_file_path = format!("{prepared_path}/fuse.link");
            let link_content = tokio_fs_ext::read_to_string(&link_file_path).await?;
            let target_dir = link_content.lines().next().unwrap_or("").trim();
            console::log_1(&format!("target_dir: {target_dir}").into());
            if !target_dir.is_empty() {
                return Box::pin(self::read_dir(target_dir)).await;
            }
        }

        Ok(entries)
    }

    /// Create directory (including parent directories)
    pub async fn write_dir(path: &str) -> Result<()> {
        tokio_fs_ext::create_dir_all(path).await?;
        Ok(())
    }

    /// Remove directory and its contents
    pub async fn remove_dir(path: &str) -> Result<()> {
        tokio_fs_ext::remove_dir_all(path).await?;
        Ok(())
    }

    /// Remove directory and its contents
    pub async fn copy(src: &str, dst: &str) -> Result<()> {
        tokio_fs_ext::copy(src, dst).await?;
        Ok(())
    }

    /// Get canonical path
    pub async fn canonicalize(path: &str) -> Result<String> {
        let canonical_path = tokio_fs_ext::canonicalize(path).await?;
        if let Some(path_str) = canonical_path.to_str() {
            Ok(path_str.to_string())
        } else {
            Err(anyhow::anyhow!("Invalid path encoding"))
        }
    }

    /// Check if file or directory exists
    pub async fn exists(path: &str) -> Result<bool> {
        match tokio_fs_ext::metadata(path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

pub mod cwd {
    use super::*;

    // FIXME: This is not thread-safe, we need to use a thread-local variable instead
    /// Set current working directory
    pub async fn set_cwd(path: &str) -> Result<()> {
        console::log_1(&format!("set_cwd() called with: {path}").into());

        if let Some(cwd) = CWD.get() {
            console::log_1(&"CWD already initialized, updating value".into());
            let mut guard = cwd.lock().unwrap();
            console::log_1(&format!("Previous CWD: {}", *guard).into());
            *guard = path.to_string();
            console::log_1(&format!("New CWD set to: {}", *guard).into());
        } else {
            console::log_1(&"CWD not initialized, creating with new value".into());
            let cwd = CWD.get_or_init(|| {
                console::log_1(&"Initializing CWD with new value".into());
                Mutex::new("/utoo-wasm-demo".to_string())
            });
            console::log_1(&format!("CWD initialized with: {}", cwd.lock().unwrap()).into());
        }
        Ok(())
    }

    /// Read current working directory
    pub async fn get_cwd() -> Result<String> {
        console::log_1(&"get_cwd() called".into());

        if let Some(cwd) = CWD.get() {
            console::log_1(&"CWD already initialized".into());
            let current_cwd = cwd.lock().unwrap().clone();
            console::log_1(&format!("Getting CWD: {current_cwd}").into());
            Ok(current_cwd)
        } else {
            console::log_1(&"CWD not initialized, creating default".into());
            let cwd = CWD.get_or_init(|| {
                console::log_1(&"Initializing CWD with default value in get_cwd".into());
                Mutex::new(String::from("/utoo-wasm-demo"))
            });
            let current_cwd = cwd.lock().unwrap().clone();
            console::log_1(&format!("Getting CWD: {current_cwd}").into());
            Ok(current_cwd)
        }
    }
}

pub mod package_manager;
