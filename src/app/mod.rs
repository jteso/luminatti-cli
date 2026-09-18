//! Application state and worktree refresh orchestration.
//! Input, navigation, and rendering stay private to this module.
use crate::{
    comments::{AgentCommentFile, CommentStore, ReviewComment},
    diff::DiffRow,
    diff_view::DiffMode,
    filters::{FilterStore, compile_filters, partition_filtered_files},
    git::{FileItem, RemoteStatus, changed_files, remote_status},
    settings::ProjectSettings,
    storage::read_json,
};
use anyhow::Result;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

mod diff;
mod files;
mod keyboard;
mod layout;
mod mouse;
mod navigation;
mod review;
mod terminal;
mod ui;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub(crate) fn run(repo: PathBuf) -> Result<()> {
    terminal::run_tui(App::new(repo)?)
}

const SCROLLBAR_VISIBILITY_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RightTab {
    Diff,
    Comments,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilterTab {
    Filters,
    Ignored,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Files,
    Right,
    Filters,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputKind {
    Comment,
    Filter,
    FileSearch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfirmationKind {
    DeleteAllComments,
}

#[derive(Debug)]
struct Input {
    kind: InputKind,
    value: String,
    selected: usize,
}

struct App {
    repo: PathBuf,
    files: Vec<FileItem>,
    ignored_files: Vec<FileItem>,
    filters: FilterStore,
    local_comments: CommentStore,
    agent_comments: Vec<ReviewComment>,
    right_tab: RightTab,
    filter_tab: FilterTab,
    focus: Focus,
    maximized_panel: Option<Focus>,
    diff_mode: DiffMode,
    show_unchanged: bool,
    final_view: bool,
    final_rows: Vec<DiffRow>,
    final_origin: Option<(usize, usize, u16)>,
    selected_file: usize,
    selected_filter: usize,
    selected_ignored: usize,
    selected_comment: usize,
    selected_row: usize,
    collapsed_dirs: BTreeSet<String>,
    divider: u16,
    dragging_divider: bool,
    diff_rows: Vec<DiffRow>,
    diff_language: String,
    diff_has_syntactic_changes: bool,
    diff_scroll: u16,
    diff_horizontal_scroll: u16,
    diff_signature: Option<u64>,
    input: Option<Input>,
    confirmation: Option<ConfirmationKind>,
    show_help: bool,
    message: String,
    remote: RemoteStatus,
    last_refresh: Instant,
    scrollbar_visible_until: Option<Instant>,
}

impl App {
    fn new(repo: PathBuf) -> Result<Self> {
        let settings: ProjectSettings =
            read_json(&repo.join(".luminatti/settings.json")).unwrap_or_default();
        let mut app = Self {
            repo,
            files: vec![],
            ignored_files: vec![],
            filters: FilterStore::default(),
            local_comments: CommentStore::default(),
            agent_comments: vec![],
            right_tab: RightTab::Diff,
            filter_tab: FilterTab::Filters,
            focus: Focus::Files,
            maximized_panel: None,
            diff_mode: settings.diff_mode,
            show_unchanged: settings.show_unchanged,
            final_view: false,
            final_rows: vec![],
            final_origin: None,
            selected_file: 0,
            selected_filter: 0,
            selected_ignored: 0,
            selected_comment: 0,
            selected_row: 0,
            collapsed_dirs: BTreeSet::new(),
            divider: settings.divider,
            dragging_divider: false,
            diff_rows: vec![],
            diff_language: String::new(),
            diff_has_syntactic_changes: false,
            diff_scroll: 0,
            diff_horizontal_scroll: 0,
            diff_signature: None,
            input: None,
            confirmation: None,
            show_help: false,
            message: "watching worktree".into(),
            remote: RemoteStatus::default(),
            last_refresh: Instant::now() - Duration::from_secs(1),
            scrollbar_visible_until: None,
        };
        app.refresh()?;
        Ok(app)
    }

    fn focus_panel(&mut self, focus: Focus) {
        if self.focus != focus {
            self.maximized_panel = None;
            self.focus = focus;
        }
    }

    fn reveal_scrollbars(&mut self) {
        self.scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_VISIBILITY_DURATION);
    }

    fn scrollbars_visible(&self) -> bool {
        self.scrollbar_visible_until
            .is_some_and(|until| Instant::now() < until)
    }

    fn refresh(&mut self) -> Result<()> {
        self.filters = read_json(&self.filters_path()).unwrap_or_default();
        self.local_comments = read_json(&self.local_comments_path()).unwrap_or_default();
        self.agent_comments =
            match read_json::<AgentCommentFile>(&self.agent_comments_path()).unwrap_or_default() {
                AgentCommentFile::Store { comments } => comments,
                AgentCommentFile::List(comments) => comments,
                AgentCommentFile::Empty => vec![],
            };
        let filter = compile_filters(&self.filters.patterns)?;
        let (ignored_files, files) = partition_filtered_files(changed_files(&self.repo)?, &filter);
        self.files = files;
        self.ignored_files = ignored_files;
        self.selected_file = self
            .selected_file
            .min(self.file_tree_rows().len().saturating_sub(1));
        self.selected_filter = self
            .selected_filter
            .min(self.filters.patterns.len().saturating_sub(1));
        self.selected_ignored = self
            .selected_ignored
            .min(self.ignored_files.len().saturating_sub(1));
        self.selected_comment = self
            .selected_comment
            .min(self.all_comments().len().saturating_sub(1));
        self.rebuild_diff()?;
        self.remote = remote_status(&self.repo);
        self.last_refresh = Instant::now();
        Ok(())
    }
}
