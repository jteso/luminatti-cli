//! Saved glob exclusions and changed-file partitioning.
use crate::{git::FileItem, storage::store_version};
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct FilterStore {
    #[serde(default = "store_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) patterns: Vec<String>,
}
impl Default for FilterStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            patterns: vec![],
        }
    }
}

pub(crate) fn compile_filters(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid saved filter: {p}"))?);
    }
    Ok(builder.build()?)
}

pub(crate) fn partition_filtered_files(
    files: Vec<FileItem>,
    filter: &GlobSet,
) -> (Vec<FileItem>, Vec<FileItem>) {
    files
        .into_iter()
        .partition(|file| filter.is_match(&file.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignored_tab_contains_files_excluded_by_filter_globs() {
        let filter = compile_filters(&["generated/**".into()]).unwrap();
        let files = vec![
            FileItem {
                path: "src/main.rs".into(),
                status: " M".into(),
            },
            FileItem {
                path: "generated/client.rs".into(),
                status: "??".into(),
            },
        ];
        let (ignored, visible) = partition_filtered_files(files, &filter);
        assert_eq!(ignored[0].path, "generated/client.rs");
        assert_eq!(visible[0].path, "src/main.rs");
    }
}
