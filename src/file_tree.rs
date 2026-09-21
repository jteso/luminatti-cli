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
    pub(crate) label: String,
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
        let mut path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let mut label = name.clone();
        let mut current = child;

        loop {
            let is_directory = !current.children.is_empty();
            if !is_directory || collapsed_dirs.contains(&path) {
                rows.push(TreeRow {
                    path,
                    label,
                    depth,
                    file_index: current.file_index,
                    expanded: false,
                });
                break;
            }

            if current.file_index.is_none() && current.children.len() == 1 {
                let (next_name, next) = current
                    .children
                    .iter()
                    .next()
                    .expect("single-child directory has a child");
                path.push('/');
                path.push_str(next_name);
                label.push('/');
                label.push_str(next_name);
                current = next;
                continue;
            }

            rows.push(TreeRow {
                path: path.clone(),
                label,
                depth,
                file_index: current.file_index,
                expanded: true,
            });
            flatten_tree(current, &path, depth + 1, collapsed_dirs, rows);
            break;
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
        let files = ["src/main.rs", "src/lib.rs"]
            .into_iter()
            .map(|path| FileItem {
                path: path.into(),
                status: " M".into(),
            })
            .collect::<Vec<_>>();
        let expanded = file_tree_rows(&files, &BTreeSet::new());
        assert_eq!(expanded.len(), 3);
        assert!(expanded[0].expanded);
        assert_eq!(expanded[1].path, "src/lib.rs");
        assert_eq!(expanded[1].file_index, Some(1));

        let collapsed = file_tree_rows(&files, &BTreeSet::from(["src".to_owned()]));
        assert_eq!(collapsed.len(), 1);
        assert!(!collapsed[0].expanded);
    }

    #[test]
    fn changed_tree_compacts_a_single_file_path_into_one_row() {
        let files = vec![FileItem {
            path: "packages/business/src/services/finance/payroll-journal/examples/report.json"
                .into(),
            status: "??".into(),
        }];

        let rows = file_tree_rows(&files, &BTreeSet::new());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, files[0].path);
        assert_eq!(rows[0].label, files[0].path);
        assert_eq!(rows[0].file_index, Some(0));
    }

    #[test]
    fn changed_tree_compacts_single_child_paths_around_a_branch() {
        let files = [
            "packages/business/src/main.rs",
            "packages/business/tests/main.rs",
        ]
        .into_iter()
        .map(|path| FileItem {
            path: path.into(),
            status: " M".into(),
        })
        .collect::<Vec<_>>();

        let rows = file_tree_rows(&files, &BTreeSet::new());
        let labels = rows
            .iter()
            .map(|row| (row.label.as_str(), row.depth, row.file_index))
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                ("packages/business", 0, None),
                ("src/main.rs", 1, Some(0)),
                ("tests/main.rs", 1, Some(1)),
            ]
        );
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
