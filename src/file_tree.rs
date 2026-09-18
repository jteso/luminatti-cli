//! Changed-file tree construction and path-ordered navigation.
use crate::git::FileItem;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct TreeNode {
    children: BTreeMap<String, TreeNode>,
    file_index: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct TreeRow {
    pub(crate) path: String,
    pub(crate) depth: usize,
    pub(crate) file_index: Option<usize>,
    pub(crate) expanded: bool,
}

pub(crate) fn file_tree_rows(
    files: &[FileItem],
    collapsed_dirs: &BTreeSet<String>,
) -> Vec<TreeRow> {
    let mut root = TreeNode::default();
    for (file_index, file) in files.iter().enumerate() {
        let mut node = &mut root;
        for segment in file.path.split('/') {
            node = node.children.entry(segment.to_owned()).or_default();
        }
        node.file_index = Some(file_index);
    }
    let mut rows = vec![];
    flatten_tree(&root, "", 0, collapsed_dirs, &mut rows);
    rows
}
fn flatten_tree(
    node: &TreeNode,
    prefix: &str,
    depth: usize,
    collapsed_dirs: &BTreeSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    for (name, child) in &node.children {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let is_directory = !child.children.is_empty();
        let expanded = is_directory && !collapsed_dirs.contains(&path);
        rows.push(TreeRow {
            path: path.clone(),
            depth,
            file_index: child.file_index,
            expanded,
        });
        if is_directory && expanded {
            flatten_tree(child, &path, depth + 1, collapsed_dirs, rows);
        }
    }
}

pub(crate) fn adjacent_file_index(
    files: &[FileItem],
    current_file: Option<usize>,
    forward: bool,
) -> Option<usize> {
    let mut indices = (0..files.len()).collect::<Vec<_>>();
    indices.sort_by(|left, right| files[*left].path.cmp(&files[*right].path));
    let position =
        current_file.and_then(|current| indices.iter().position(|index| *index == current));
    match (position, forward) {
        (Some(position), true) => indices.get(position + 1).copied(),
        (Some(position), false) => position
            .checked_sub(1)
            .and_then(|previous| indices.get(previous).copied()),
        (None, true) => indices.first().copied(),
        (None, false) => indices.last().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_tree_hides_collapsed_descendants() {
        let files = vec![FileItem {
            path: "src/main.rs".into(),
            status: " M".into(),
        }];
        let expanded = file_tree_rows(&files, &BTreeSet::new());
        assert_eq!(expanded.len(), 2);
        assert!(expanded[0].expanded);
        assert_eq!(expanded[1].path, "src/main.rs");
        assert_eq!(expanded[1].file_index, Some(0));

        let collapsed = file_tree_rows(&files, &BTreeSet::from(["src".to_owned()]));
        assert_eq!(collapsed.len(), 1);
        assert!(!collapsed[0].expanded);
    }

    #[test]
    fn file_navigation_follows_display_path_order() {
        let files = vec![
            FileItem {
                path: "src/b.rs".into(),
                status: " M".into(),
            },
            FileItem {
                path: "src/a.rs".into(),
                status: " M".into(),
            },
            FileItem {
                path: "src/c.rs".into(),
                status: " M".into(),
            },
        ];

        assert_eq!(adjacent_file_index(&files, Some(0), true), Some(2));
        assert_eq!(adjacent_file_index(&files, Some(0), false), Some(1));
        assert_eq!(adjacent_file_index(&files, Some(2), true), None);
        assert_eq!(adjacent_file_index(&files, Some(1), false), None);
    }
}
