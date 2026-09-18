//! In-memory fixtures: no Git commands or metadata writes.
use super::*;
use crate::diff::{line_diff_document, structural_diff_document};
use std::path::Path;

pub(super) fn test_diff_rows(before: &str, after: &str) -> Vec<DiffRow> {
    structural_diff_document(Path::new("example.txt"), before, after).rows
}

pub(super) fn preview_app(before: &str, after: &str) -> App {
    App {
        repo: PathBuf::new(),
        files: vec![],
        ignored_files: vec![],
        filters: FilterStore::default(),
        local_comments: CommentStore::default(),
        agent_comments: vec![],
        right_tab: RightTab::Diff,
        filter_tab: FilterTab::Filters,
        focus: Focus::Right,
        maximized_panel: None,
        diff_mode: DiffMode::Unified,
        show_unchanged: false,
        final_view: false,
        final_rows: line_diff_document(after, after).rows,
        final_origin: None,
        selected_file: 0,
        selected_filter: 0,
        selected_ignored: 0,
        selected_comment: 0,
        selected_row: 0,
        collapsed_dirs: BTreeSet::new(),
        divider: 34,
        dragging_divider: false,
        diff_rows: test_diff_rows(before, after),
        diff_language: String::new(),
        diff_has_syntactic_changes: true,
        diff_scroll: 0,
        diff_horizontal_scroll: 0,
        diff_signature: None,
        input: None,
        confirmation: None,
        show_help: false,
        message: String::new(),
        remote: RemoteStatus::default(),
        last_refresh: Instant::now(),
        scrollbar_visible_until: None,
    }
}
