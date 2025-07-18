use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PackageNode {
    pub name: String,
    pub version: String,
    pub path: String,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub optional_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
}

impl PackageNode {
    pub fn new(name: String, version: String, path: String) -> Self {
        Self {
            name,
            version,
            path,
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            peer_dependencies: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyType {
    Production,
    Development,
    Optional,
    Peer,
}

#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub _dependency_type: DependencyType,
    pub _version_spec: String,
}

pub struct DependencyGraphService {
    pub graph: DiGraph<PackageNode, DependencyEdge>,
    name_to_indices: HashMap<String, Vec<NodeIndex>>,
    path_to_index: HashMap<String, NodeIndex>,
}

impl Default for DependencyGraphService {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraphService {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            name_to_indices: HashMap::new(),
            path_to_index: HashMap::new(),
        }
    }
    pub fn add_package(&mut self, package: PackageNode) -> NodeIndex {
        let package_name = package.name.clone();
        let package_path = package.path.clone();
        let node_index = self.graph.add_node(package);
        self.name_to_indices
            .entry(package_name)
            .or_default()
            .push(node_index);
        self.path_to_index.insert(package_path, node_index);
        node_index
    }
    /// Try to add dependency edge, return error if not found
    pub fn try_add_dependency_with_path(
        &mut self,
        from_path: &str,
        _from_package_name: &str,
        to_package_name: &str,
        dependency_type: DependencyType,
        version_spec: String,
    ) -> Result<(), String> {
        let from_index = self
            .path_to_index
            .get(from_path)
            .ok_or_else(|| format!("Package at path '{from_path}' not found in graph"))?;
        let to_index = self
            .find_dependency_node_index(from_path, to_package_name)
            .ok_or_else(|| {
                format!("Dependency package '{to_package_name}' not found for path '{from_path}'")
            })?;
        let edge = DependencyEdge {
            _dependency_type: dependency_type,
            _version_spec: version_spec,
        };
        self.graph.add_edge(to_index, *from_index, edge);
        Ok(())
    }
    pub fn find_dependency_node_index(
        &self,
        current_path: &str,
        dependency_name: &str,
    ) -> Option<NodeIndex> {
        let candidate_indices = self.name_to_indices.get(dependency_name)?;
        if candidate_indices.is_empty() {
            return None;
        }
        let preferred_path = if current_path.is_empty() {
            format!("node_modules/{dependency_name}")
        } else {
            format!("{current_path}/node_modules/{dependency_name}")
        };
        if let Some(&preferred_index) = self.path_to_index.get(&preferred_path) {
            return Some(preferred_index);
        }
        let mut search_path = current_path.to_string();
        loop {
            let candidate_path = if search_path.is_empty() {
                format!("node_modules/{dependency_name}")
            } else {
                format!("{search_path}/node_modules/{dependency_name}")
            };
            if let Some(&index) = self.path_to_index.get(&candidate_path) {
                return Some(index);
            }
            if search_path.is_empty() {
                break;
            }
            if let Some(last_slash) = search_path.rfind('/') {
                search_path = search_path[..last_slash].to_string();
            } else {
                search_path.clear();
            }
        }
        candidate_indices.first().copied()
    }

    /// Get dependency tree for a specific package as nested structure
    pub fn get_package_dependency_tree(&self, package_name: &str) -> Option<DepTreeNode> {
        let node_paths = self.find_paths_to_root(package_name);
        if node_paths.is_empty() {
            return None;
        }
        let tree = build_dep_tree(&node_paths);
        Some(tree)
    }

    /// Find all paths from package to root
    pub fn find_paths_to_root(&self, package_name: &str) -> Vec<Vec<NodeIndex>> {
        let package_indices = if let Some(indices) = self.name_to_indices.get(package_name) {
            indices
        } else {
            return Vec::new();
        };
        let mut all_paths = Vec::new();
        // Root node is the one with empty path
        if let Some(&root_index) = self.path_to_index.get("") {
            for &package_index in package_indices {
                let mut path = Vec::new();
                self.dfs_all_paths(
                    package_index,
                    root_index,
                    &mut Vec::new(),
                    &mut path,
                    &mut all_paths,
                );
            }
        }
        all_paths
    }

    fn dfs_all_paths(
        &self,
        current: NodeIndex,
        target: NodeIndex,
        visited: &mut Vec<NodeIndex>,
        path: &mut Vec<NodeIndex>,
        all_paths: &mut Vec<Vec<NodeIndex>>,
    ) {
        if visited.contains(&current) {
            return;
        }
        visited.push(current);
        path.push(current);
        if current == target {
            all_paths.push(path.clone());
        } else {
            for neighbor in self.graph.neighbors(current) {
                self.dfs_all_paths(neighbor, target, visited, path, all_paths);
            }
        }
        path.pop();
        visited.pop();
    }

    /// Generate tree-like text lines for a package's dependency tree
    pub fn print_dep_tree_lines(
        &self,
        node: &DepTreeNode,
        prefix: &str,
        is_last: bool,
        highlight: &[&str],
        lines: &mut Vec<String>,
    ) {
        let is_root = node.index == petgraph::graph::NodeIndex::end();
        if !is_root {
            let branch = if is_last {
                "└──"
            } else {
                "├───┬"
            };
            if let Some(pkg) = self.graph.node_weight(node.index) {
                let is_highlight = highlight.iter().any(|&h| pkg.name.starts_with(h));
                let display = format!("{}@{}", pkg.name, pkg.version);
                if is_highlight {
                    if !pkg.path.is_empty() {
                        lines.push(format!("{}{} {} -> {}", prefix, branch, display, pkg.path));
                    } else {
                        lines.push(format!("{prefix}{branch} {display}"));
                    }
                } else if !pkg.path.is_empty() {
                    lines.push(format!("{}{} {} -> {}", prefix, branch, display, pkg.path));
                } else {
                    lines.push(format!("{prefix}{branch} {display}"));
                }
            }
        }
        let len = node.children.len();
        for (i, child) in node.children.values().enumerate() {
            let is_last_child = i == len - 1;
            let new_prefix = if is_root {
                String::new()
            } else {
                format!("{}{}", prefix, if is_last { "    " } else { "│   " })
            };
            self.print_dep_tree_lines(child, &new_prefix, is_last_child, highlight, lines);
        }
    }

    /// Show package dependencies as tree-like text lines
    pub fn show_package_dependencies(&self, package_name: &str) -> Option<Vec<String>> {
        let node_paths = self.find_paths_to_root(package_name);
        if node_paths.is_empty() {
            return None;
        }
        let tree = build_dep_tree(&node_paths);
        let mut lines = Vec::new();
        self.print_dep_tree_lines(&tree, "", true, &[package_name], &mut lines);
        Some(lines)
    }
}

/// Dependency tree node for output
#[derive(Debug, Clone)]
pub struct DepTreeNode {
    pub index: NodeIndex,
    pub children: std::collections::BTreeMap<NodeIndex, DepTreeNode>,
}

/// Build dependency tree from all paths
pub fn build_dep_tree(paths: &[Vec<NodeIndex>]) -> DepTreeNode {
    let mut root = DepTreeNode {
        index: NodeIndex::end(),
        children: std::collections::BTreeMap::new(),
    };
    for path in paths {
        let mut node = &mut root;
        for &idx in path.iter().rev() {
            node = node.children.entry(idx).or_insert_with(|| DepTreeNode {
                index: idx,
                children: std::collections::BTreeMap::new(),
            });
        }
    }
    root
}
