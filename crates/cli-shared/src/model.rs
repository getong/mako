use crate::dependency_graph::{DependencyGraphService, DependencyType, PackageNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents package information in package-lock.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockPackage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies", skip_serializing_if = "Option::is_none")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies", skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        rename = "optionalDependencies",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(rename = "hasInstallScript", skip_serializing_if = "Option::is_none")]
    pub has_install_script: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,
}

impl LockPackage {
    /// Get package name, infer from path if not available
    pub fn get_name(&self, path: &str) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if path.is_empty() {
            "root".to_string()
        } else {
            // Extract package name from path
            path.rsplit('/').next().unwrap_or("unknown").to_string()
        }
    }
    /// Get package version
    pub fn get_version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }
}

/// Represents complete package-lock.json file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLock {
    pub name: String,
    pub version: String,
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,
    pub requires: bool,
    pub packages: HashMap<String, LockPackage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, serde_json::Value>>,
}

impl PackageLock {
    /// Parse from json string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
    /// Parse from serde_json::Value
    pub fn from_value(value: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value.clone())
    }

    /// Build dependency graph from the package lock data
    pub fn build_dependency_graph(&self) -> DependencyGraphService {
        let mut graph = DependencyGraphService::new();
        // Add all package nodes
        for (path, package) in &self.packages {
            let name = package.get_name(path);
            let version = package.get_version();
            let mut node = PackageNode::new(name.clone(), version.clone(), path.clone());
            if let Some(deps) = &package.dependencies {
                node.dependencies = deps.clone();
            }
            if let Some(dev_deps) = &package.dev_dependencies {
                node.dev_dependencies = dev_deps.clone();
            }
            if let Some(peer_deps) = &package.peer_dependencies {
                node.peer_dependencies = peer_deps.clone();
            }
            if let Some(opt_deps) = &package.optional_dependencies {
                node.optional_dependencies = opt_deps.clone();
            }
            graph.add_package(node);
        }
        // Add dependency edges
        for (path, package) in &self.packages {
            let name = package.name.clone().unwrap_or_else(|| "root".to_string());
            // Production dependencies
            if let Some(deps) = &package.dependencies {
                for (dep_name, dep_version) in deps {
                    let _ = graph.try_add_dependency_with_path(
                        path,
                        &name,
                        dep_name,
                        DependencyType::Production,
                        dep_version.clone(),
                    );
                }
            }
            // Development dependencies
            if let Some(dev_deps) = &package.dev_dependencies {
                for (dep_name, dep_version) in dev_deps {
                    let _ = graph.try_add_dependency_with_path(
                        path,
                        &name,
                        dep_name,
                        DependencyType::Development,
                        dep_version.clone(),
                    );
                }
            }
            // Optional dependencies
            if let Some(opt_deps) = &package.optional_dependencies {
                for (dep_name, dep_version) in opt_deps {
                    let _ = graph.try_add_dependency_with_path(
                        path,
                        &name,
                        dep_name,
                        DependencyType::Optional,
                        dep_version.clone(),
                    );
                }
            }
            // Peer dependencies
            if let Some(peer_deps) = &package.peer_dependencies {
                for (dep_name, dep_version) in peer_deps {
                    let _ = graph.try_add_dependency_with_path(
                        path,
                        &name,
                        dep_name,
                        DependencyType::Peer,
                        dep_version.clone(),
                    );
                }
            }
        }
        graph
    }
}
