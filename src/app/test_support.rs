//! In-memory fixtures: no Git commands or metadata writes.
use super::*;
use crate::diff::{line_diff_document, structural_diff_document};
use std::{cell::RefCell, path::Path};

pub(super) fn test_diff_rows(before: &str, after: &str) -> Vec<DiffRow> {
    structural_diff_document(Path::new("example.txt"), before, after).rows
}

pub(super) fn wait_for_diff(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.pending_diff.is_some() {
        app.poll_diff();
        assert!(Instant::now() < deadline, "diff worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
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
        final_rows: line_diff_document(after, after).rows.into(),
        final_origin: None,
        selected_file: 0,
        selected_filter: 0,
        selected_ignored: 0,
        active_file_list: ActiveFileList::Changed,
        selected_comment: 0,
        selected_row: 0,
        diff_selection_active: true,
        collapsed_dirs: BTreeSet::new(),
        divider: 34,
        dragging_divider: false,
        diff_rows: test_diff_rows(before, after).into(),
        diff_language: String::new(),
        diff_has_syntactic_changes: true,
        diff_scroll: 0,
        diff_horizontal_scroll: 0,
        diff_signature: None,
        active_diff_path: None,
        diff_worker: None,
        pending_diff: None,
        loaded_diff: None,
        diff_anchor: None,
        select_last_change: false,
        render_state_slot: RefCell::new(None),
        input: None,
        confirmation: None,
        show_help: false,
        message: String::new(),
        remote: RemoteStatus::default(),
        last_refresh: Instant::now(),
        refresh_worker: None,
        refresh_pending: false,
        scrollbar_visible_until: None,
    }
}
