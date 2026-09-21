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
    cell::RefCell,
    collections::BTreeSet,
    path::PathBuf,
    sync::Arc,
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
mod worker;

use self::diff::{DiffRenderState, DiffWorker, LoadedDiff, PendingDiff};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub(crate) fn run(repo: PathBuf) -> Result<()> {
    terminal::run_tui(App::new(repo)?)
}

const SCROLLBAR_VISIBILITY_DURATION: Duration = Duration::from_secs(3);
const CURSOR_BLINK_MILLIS: u128 = 530;

/// Whether the block cursor should currently be drawn, toggling over time.
pub(crate) fn cursor_blink_visible() -> bool {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    (millis / CURSOR_BLINK_MILLIS).is_multiple_of(2)
}

type RefreshWorker = worker::Worker<(), Result<(Vec<FileItem>, RemoteStatus)>>;

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
enum ActiveFileList {
    Changed,
    Ignored,
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
    final_rows: Arc<Vec<DiffRow>>,
    final_origin: Option<(usize, usize, usize)>,
    selected_file: usize,
    selected_filter: usize,
    selected_ignored: usize,
    active_file_list: ActiveFileList,
    selected_comment: usize,
    selected_row: usize,
    diff_selection_active: bool,
    collapsed_dirs: BTreeSet<String>,
    divider: u16,
    dragging_divider: bool,
    diff_rows: Arc<Vec<DiffRow>>,
    diff_language: String,
    diff_has_syntactic_changes: bool,
    diff_scroll: usize,
    diff_horizontal_scroll: u16,
    diff_signature: Option<u64>,
    /// Path of the file whose diff is currently rendered, if any.
    active_diff_path: Option<String>,
    diff_worker: Option<DiffWorker>,
    pending_diff: Option<PendingDiff>,
    loaded_diff: Option<Arc<LoadedDiff>>,
    diff_anchor: Option<(Option<u32>, Option<u32>)>,
    select_last_change: bool,
    render_state_slot: RefCell<Option<Arc<DiffRenderState>>>,
    input: Option<Input>,
    confirmation: Option<ConfirmationKind>,
    show_help: bool,
    message: String,
    remote: RemoteStatus,
    last_refresh: Instant,
    refresh_worker: Option<RefreshWorker>,
    refresh_pending: bool,
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
            final_rows: Arc::default(),
            final_origin: None,
            selected_file: 0,
            selected_filter: 0,
            selected_ignored: 0,
            active_file_list: ActiveFileList::Changed,
            selected_comment: 0,
            selected_row: 0,
            diff_selection_active: false,
            collapsed_dirs: BTreeSet::new(),
            divider: settings.divider,
            dragging_divider: false,
            diff_rows: Arc::default(),
            diff_language: String::new(),
            diff_has_syntactic_changes: false,
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
            message: "watching worktree".into(),
            remote: RemoteStatus::default(),
            last_refresh: Instant::now() - Duration::from_secs(1),
            refresh_worker: None,
            refresh_pending: false,
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
        if self.refresh_worker.is_none() {
            let repo = self.repo.clone();
            self.refresh_worker = Some(worker::Worker::spawn("worktree-refresh", move |()| {
                Ok((changed_files(&repo)?, remote_status(&repo)))
            })?);
        }
        if !self.refresh_pending {
            self.refresh_worker
                .as_mut()
                .expect("worker initialized")
                .request(());
            self.refresh_pending = true;
        }
        self.last_refresh = Instant::now();
        Ok(())
    }

    fn poll_refresh(&mut self) -> Result<bool> {
        let Some((_, result)) = self.refresh_worker.as_ref().and_then(worker::Worker::take) else {
            return Ok(false);
        };
        self.refresh_pending = false;
        let (files, remote) = result?;
        let previous_path = self.active_path().map(str::to_owned);
        let previous_file_list = self.active_file_list;
        let filter = compile_filters(&self.filters.patterns)?;
        let (ignored_files, files) = partition_filtered_files(files, &filter);
        self.files = files;
        self.ignored_files = ignored_files;
        self.selected_file = self
            .selected_file
            .min(self.file_tree_rows().len().saturating_sub(1));
        if let Some(path) = previous_path {
            match previous_file_list {
                ActiveFileList::Changed => {
                    if let Some(index) = self
                        .file_tree_rows()
                        .iter()
                        .position(|row| row.path == path)
                    {
                        self.selected_file = index;
                    }
                }
                ActiveFileList::Ignored => {
                    if let Some(index) =
                        self.ignored_files.iter().position(|file| file.path == path)
                    {
                        self.selected_ignored = index;
                    }
                }
            }
        }
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
        self.remote = remote;
        Ok(true)
    }
}
