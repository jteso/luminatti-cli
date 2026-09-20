//! Git worktree discovery, changed files, and branch status.
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug)]
pub(crate) struct FileItem {
    pub(crate) path: String,
    pub(crate) status: String,
}
#[derive(Default)]
pub(crate) struct RemoteStatus {
    pub(crate) branch: String,
    pub(crate) behind: Option<u32>,
    pub(crate) ahead: Option<u32>,
}
fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?)
}
fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git(repo, args)?;
    if !output.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
pub(crate) fn find_repo(dir: &Path) -> Result<PathBuf> {
    let dir = fs::canonicalize(dir).with_context(|| format!("cannot access {}", dir.display()))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        bail!("{} is not inside a Git worktree", dir.display());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}
pub(crate) fn changed_files(repo: &Path) -> Result<Vec<FileItem>> {
    let output = git(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let records: Vec<_> = output.stdout.split(|b| *b == 0).collect();
    let mut seen = BTreeSet::new();
    let mut files = vec![];
    let mut index = 0;
    while index < records.len() {
        let entry = records[index];
        if entry.len() < 4 {
            index += 1;
            continue;
        }
        let status = String::from_utf8_lossy(&entry[..2]).to_string();
        let path = String::from_utf8_lossy(&entry[3..]).to_string();
        if !is_luminatti_metadata(&path) && seen.insert(path.clone()) {
            files.push(FileItem { path, status });
        }
        // A renamed/copied porcelain v1 record has a second NUL-delimited
        // source path without a status prefix. It is not another file entry.
        index += if matches!(entry.first(), Some(b'R' | b'C')) {
            2
        } else {
            1
        };
    }
    Ok(files)
}
fn is_luminatti_metadata(path: &str) -> bool {
    path == ".luminatti" || path.starts_with(".luminatti/")
}
/// Current commit of `HEAD`, or an empty string before the first commit.
pub(crate) fn head_commit(repo: &Path) -> String {
    git_text(repo, &["rev-parse", "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string()
}
pub(crate) fn git_show_head_file(repo: &Path, path: &str) -> Result<String> {
    let spec = format!("HEAD:{path}");
    let output = git(repo, &["show", &spec])?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}
pub(crate) fn remote_status(repo: &Path) -> RemoteStatus {
    let branch = git_text(repo, &["branch", "--show-current"])
        .unwrap_or_else(|_| "detached".into())
        .trim()
        .to_string();
    let branch = if branch.is_empty() {
        "detached".into()
    } else {
        branch
    };
    match git_text(
        repo,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    ) {
        Ok(counts) => {
            let parts: Vec<_> = counts.split_whitespace().collect();
            RemoteStatus {
                branch,
                behind: parts.first().and_then(|count| count.parse().ok()),
                ahead: parts.get(1).and_then(|count| count.parse().ok()),
            }
        }
        Err(_) => RemoteStatus {
            branch,
            behind: None,
            ahead: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn luminatti_metadata_is_not_shown_as_a_reviewable_change() {
        assert!(is_luminatti_metadata(".luminatti/settings.json"));
        assert!(is_luminatti_metadata(".luminatti/comments.json"));
        assert!(!is_luminatti_metadata("src/.luminatti/settings.json"));
        assert!(!is_luminatti_metadata(".luminatti-example"));
    }
}
