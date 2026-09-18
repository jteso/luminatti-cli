use super::{App, Focus, RightTab};
use crate::file_tree::{TreeRow, file_tree_rows};
use anyhow::Result;

impl App {
    pub(super) fn active_path(&self) -> Option<&str> {
        self.active_file_index()
            .and_then(|index| self.files.get(index))
            .map(|file| file.path.as_str())
    }

    pub(super) fn file_tree_rows(&self) -> Vec<TreeRow> {
        file_tree_rows(&self.files, &self.collapsed_dirs)
    }
    pub(super) fn active_file_index(&self) -> Option<usize> {
        self.file_tree_rows()
            .get(self.selected_file)
            .and_then(|row| row.file_index)
    }

    pub(super) fn select_file(&mut self, file_index: usize) -> Result<()> {
        let Some(path) = self.files.get(file_index).map(|file| file.path.clone()) else {
            return Ok(());
        };
        self.collapsed_dirs.retain(|directory| {
            !path
                .strip_prefix(directory)
                .is_some_and(|suffix| suffix.starts_with('/'))
        });
        if let Some(row_index) = self
            .file_tree_rows()
            .iter()
            .position(|row| row.file_index == Some(file_index))
        {
            self.selected_file = row_index;
            self.right_tab = RightTab::Diff;
            self.focus_panel(Focus::Right);
            self.rebuild_diff()?;
        }
        Ok(())
    }

    pub(super) fn toggle_selected_directory(&mut self) {
        let Some(row) = self.file_tree_rows().get(self.selected_file).cloned() else {
            return;
        };
        if row.file_index.is_some() {
            return;
        }
        if !self.collapsed_dirs.insert(row.path.clone()) {
            self.collapsed_dirs.remove(&row.path);
        }
        self.selected_file = self
            .selected_file
            .min(self.file_tree_rows().len().saturating_sub(1));
    }
}
