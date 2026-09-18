//! Diff rebuilding and source anchors across display modes.
use super::{
    App,
    navigation::{clamped_diff_scroll, nearest_source_row},
};
use crate::{
    diff::{DiffRow, line_diff_document, structural_diff_document},
    diff_view::{DiffMode, RenderedDiffLine, displayed_diff_indices, rendered_diff_lines},
    git::git_show_head_file,
};
use anyhow::Result;
use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
};

impl App {
    pub(super) fn rebuild_diff(&mut self) -> Result<()> {
        let Some(path) = self.active_path().map(str::to_owned) else {
            self.diff_rows.clear();
            self.final_rows.clear();
            self.final_origin = None;
            self.diff_language.clear();
            self.diff_has_syntactic_changes = false;
            self.diff_horizontal_scroll = 0;
            self.diff_signature = None;
            return Ok(());
        };
        let before = git_show_head_file(&self.repo, &path)?;
        let after = fs::read_to_string(self.repo.join(&path)).unwrap_or_default();
        let mut signature_hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut signature_hasher);
        before.hash(&mut signature_hasher);
        after.hash(&mut signature_hasher);
        let signature = signature_hasher.finish();
        if self.diff_signature == Some(signature) {
            return Ok(());
        }
        self.diff_rows.clear();
        self.diff_scroll = 0;
        self.diff_horizontal_scroll = 0;
        let document = match self.diff_mode {
            DiffMode::SideBySide => line_diff_document(&before, &after),
            DiffMode::Unified => structural_diff_document(Path::new(&path), &before, &after),
        };
        self.diff_rows = document.rows;
        // Use physical source lines: structural alignment may rearrange rows.
        self.final_rows = line_diff_document(&after, &after).rows;
        self.final_origin = None;
        self.diff_language = document.language;
        self.diff_has_syntactic_changes = document.has_syntactic_changes;
        self.selected_row = self
            .diff_rows
            .iter()
            .position(DiffRow::is_changed)
            .unwrap_or(0);
        if self.final_view {
            self.selected_row = nearest_source_row(
                &self.final_rows,
                self.diff_rows
                    .get(self.selected_row)
                    .and_then(|row| row.new_line),
            );
        }
        self.diff_signature = Some(signature);
        Ok(())
    }

    pub(super) fn set_diff_mode(&mut self, mode: DiffMode) -> Result<()> {
        if self.diff_mode == mode {
            return Ok(());
        }
        let anchor = self
            .active_rows()
            .get(self.selected_row)
            .map(|row| (row.old_line, row.new_line));
        self.diff_mode = mode;
        self.final_view = false;
        self.final_origin = None;
        self.diff_signature = None;
        self.rebuild_diff()?;
        if let Some((old_line, new_line)) = anchor {
            self.selected_row = self
                .diff_rows
                .iter()
                .position(|row| row.old_line == old_line && row.new_line == new_line)
                .or_else(|| {
                    new_line.and_then(|line| {
                        self.diff_rows
                            .iter()
                            .position(|row| row.new_line == Some(line))
                    })
                })
                .or_else(|| {
                    old_line.and_then(|line| {
                        self.diff_rows
                            .iter()
                            .position(|row| row.old_line == Some(line))
                    })
                })
                .unwrap_or(self.selected_row);
        }
        self.diff_scroll = 0;
        Ok(())
    }

    pub(super) fn active_rows(&self) -> &[DiffRow] {
        if self.final_view {
            &self.final_rows
        } else {
            &self.diff_rows
        }
    }

    pub(super) fn displayed_indices(&self) -> Vec<usize> {
        displayed_diff_indices(
            self.active_rows(),
            self.final_view || self.show_unchanged,
            self.diff_mode,
        )
    }

    pub(super) fn rendered_lines(&self) -> Vec<RenderedDiffLine> {
        rendered_diff_lines(
            self.active_rows(),
            self.final_view || self.show_unchanged,
            self.diff_mode,
        )
    }

    pub(super) fn toggle_final_view(&mut self, viewport_height: u16) {
        let rendered = self.rendered_lines();
        let selected_position = rendered
            .iter()
            .position(|line| line.row_index() == Some(self.selected_row))
            .unwrap_or(0);
        let offset = selected_position
            .saturating_sub(self.diff_scroll as usize)
            .min(viewport_height.saturating_sub(1) as usize);
        let anchor = self
            .active_rows()
            .get(self.selected_row)
            .and_then(|row| row.new_line)
            .or_else(|| {
                self.active_rows()
                    .iter()
                    .skip(self.selected_row)
                    .find_map(|row| row.new_line)
            })
            .or_else(|| {
                self.active_rows()
                    .iter()
                    .take(self.selected_row)
                    .rev()
                    .find_map(|row| row.new_line)
            });
        if !self.final_view {
            let target = nearest_source_row(&self.final_rows, anchor);
            self.final_origin = Some((self.selected_row, target, self.diff_scroll));
            self.final_view = true;
            self.selected_row = target;
        } else {
            self.final_view = false;
            if let Some((original, target, scroll)) = self.final_origin.take()
                && self.selected_row == target
            {
                self.selected_row = original;
                self.diff_scroll = scroll;
                return;
            }
            let visible = self.displayed_indices();
            self.selected_row = visible
                .into_iter()
                .min_by_key(|&index| {
                    self.diff_rows[index]
                        .new_line
                        .map_or(u32::MAX, |line| line.abs_diff(anchor.unwrap_or(1)))
                })
                .unwrap_or(0);
        }
        let rendered = self.rendered_lines();
        let position = rendered
            .iter()
            .position(|line| line.row_index() == Some(self.selected_row))
            .unwrap_or(0);
        self.diff_scroll = clamped_diff_scroll(
            position.saturating_sub(offset).min(u16::MAX as usize) as u16,
            rendered.len(),
            viewport_height,
        );
    }
}
