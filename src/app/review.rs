//! Project metadata paths and comment/filter actions.
use super::App;
use crate::{
    comments::{ReviewComment, new_id, remove_local_comment},
    settings::ProjectSettings,
    storage::save_json,
};
use anyhow::{Context, Result, bail};
use arboard::Clipboard;
use globset::Glob;
use std::path::PathBuf;

impl App {
    pub(super) fn metadata_dir(&self) -> PathBuf {
        self.repo.join(".luminatti")
    }
    pub(super) fn filters_path(&self) -> PathBuf {
        self.metadata_dir().join("filters.json")
    }
    pub(super) fn local_comments_path(&self) -> PathBuf {
        self.metadata_dir().join("comments.json")
    }
    pub(super) fn agent_comments_path(&self) -> PathBuf {
        self.metadata_dir().join("agent-comments.json")
    }
    pub(super) fn settings_path(&self) -> PathBuf {
        self.metadata_dir().join("settings.json")
    }
    pub(super) fn all_comments(&self) -> Vec<&ReviewComment> {
        self.local_comments
            .comments
            .iter()
            .chain(&self.agent_comments)
            .collect()
    }

    pub(super) fn save_filters(&self) -> Result<()> {
        save_json(&self.filters_path(), &self.filters)
    }
    pub(super) fn save_comments(&self) -> Result<()> {
        save_json(&self.local_comments_path(), &self.local_comments)
    }
    pub(super) fn save_settings(&self) -> Result<()> {
        save_json(
            &self.settings_path(),
            &ProjectSettings::new(self.diff_mode, self.show_unchanged, self.divider),
        )
    }

    pub(super) fn add_comment(&mut self, summary: String) -> Result<()> {
        let Some(path) = self.active_path().map(str::to_owned) else {
            bail!("choose a changed file first");
        };
        let row = self.active_rows().get(self.selected_row);
        let (old_line, new_line) = row
            .map(|r| (if self.final_view { None } else { r.old_line }, r.new_line))
            .unwrap_or((None, None));
        if old_line.is_none() && new_line.is_none() {
            bail!("choose a diff line first");
        }
        self.local_comments.comments.push(ReviewComment {
            id: new_id(),
            file_path: path,
            old_line,
            new_line,
            hunk: None,
            summary,
            rationale: None,
            author: None,
            source: "user".into(),
        });
        self.save_comments()?;
        self.message = "comment saved in .luminatti/comments.json".into();
        Ok(())
    }

    pub(super) fn add_filter(&mut self, glob: String) -> Result<()> {
        Glob::new(&glob).with_context(|| format!("invalid glob: {glob}"))?;
        if !self.filters.patterns.contains(&glob) {
            self.filters.patterns.push(glob);
            self.save_filters()?;
        }
        self.refresh()?;
        self.message = "filter saved".into();
        Ok(())
    }

    pub(super) fn copy_comment(&mut self) -> Result<()> {
        let comments = self.all_comments();
        let comment = comments
            .get(self.selected_comment)
            .context("choose a comment first")?;
        let payload = serde_json::to_string_pretty(comment)?;
        Clipboard::new()?.set_text(payload)?;
        self.message = "comment JSON copied to clipboard".into();
        Ok(())
    }

    pub(super) fn delete_selected_comment(&mut self) -> Result<()> {
        if self.all_comments().is_empty() {
            self.message = "no comments to delete".into();
            return Ok(());
        }
        if remove_local_comment(&mut self.local_comments, self.selected_comment).is_none() {
            self.message = "agent comments are read-only".into();
            return Ok(());
        }
        self.save_comments()?;
        self.selected_comment = self
            .selected_comment
            .min(self.all_comments().len().saturating_sub(1));
        self.message = "comment deleted".into();
        Ok(())
    }

    pub(super) fn delete_all_comments(&mut self) -> Result<()> {
        if self.local_comments.comments.is_empty() {
            self.message = "no local comments to delete".into();
            return Ok(());
        }
        self.local_comments.comments.clear();
        self.save_comments()?;
        self.selected_comment = 0;
        self.message = "all local comments deleted".into();
        Ok(())
    }
}
