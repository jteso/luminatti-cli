use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use arboard::Clipboard;
use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod diff;
mod diff_view;
mod settings;

#[cfg(test)]
use diff::visible_diff_indices;
use diff::{
    DiffRow, adjacent_changed_row, changed_row_indices, line_diff_document,
    structural_diff_document,
};
use diff_view::{
    DiffMode, RenderedDiffLine, SELECTION_BACKGROUND, displayed_diff_indices, pad_selected_line,
    rendered_diff_lines, side_line, split_separator_line, unified_lines,
};
use settings::ProjectSettings;

#[derive(Parser, Debug)]
#[command(version, about = "Fast syntax-aware worktree diff review")]
struct Cli {
    /// Directory inside the Git worktree to review
    #[arg(default_value = ".")]
    dir: PathBuf,
}

#[derive(Clone, Debug)]
struct FileItem {
    path: String,
    status: String,
}

#[derive(Default)]
struct TreeNode {
    children: BTreeMap<String, TreeNode>,
    file_index: Option<usize>,
}

#[derive(Clone)]
struct TreeRow {
    path: String,
    depth: usize,
    file_index: Option<usize>,
    expanded: bool,
}

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

#[derive(Default)]
struct RemoteStatus {
    branch: String,
    behind: Option<u32>,
    ahead: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewComment {
    #[serde(default = "new_id")]
    id: String,
    file_path: String,
    #[serde(default)]
    old_line: Option<u32>,
    #[serde(default)]
    new_line: Option<u32>,
    #[serde(default)]
    hunk: Option<u32>,
    summary: String,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default = "local_source")]
    source: String,
}
fn new_id() -> String {
    format!("luminatti:{}", Uuid::new_v4())
}
fn local_source() -> String {
    "user".into()
}

#[derive(Serialize, Deserialize)]
struct CommentStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    comments: Vec<ReviewComment>,
}
impl Default for CommentStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            comments: vec![],
        }
    }
}
fn store_version() -> u32 {
    1
}
#[derive(Serialize, Deserialize)]
struct FilterStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    patterns: Vec<String>,
}
impl Default for FilterStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            patterns: vec![],
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum AgentCommentFile {
    #[default]
    Empty,
    Store {
        #[serde(default)]
        comments: Vec<ReviewComment>,
    },
    List(Vec<ReviewComment>),
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
        };
        app.refresh()?;
        Ok(app)
    }

    fn metadata_dir(&self) -> PathBuf {
        self.repo.join(".luminatti")
    }
    fn filters_path(&self) -> PathBuf {
        self.metadata_dir().join("filters.json")
    }
    fn local_comments_path(&self) -> PathBuf {
        self.metadata_dir().join("comments.json")
    }
    fn agent_comments_path(&self) -> PathBuf {
        self.metadata_dir().join("agent-comments.json")
    }
    fn settings_path(&self) -> PathBuf {
        self.metadata_dir().join("settings.json")
    }
    fn active_path(&self) -> Option<&str> {
        self.active_file_index()
            .and_then(|index| self.files.get(index))
            .map(|file| file.path.as_str())
    }

    fn focus_panel(&mut self, focus: Focus) {
        if self.focus != focus {
            self.maximized_panel = None;
            self.focus = focus;
        }
    }
    fn all_comments(&self) -> Vec<&ReviewComment> {
        self.local_comments
            .comments
            .iter()
            .chain(&self.agent_comments)
            .collect()
    }

    fn file_tree_rows(&self) -> Vec<TreeRow> {
        let mut root = TreeNode::default();
        for (file_index, file) in self.files.iter().enumerate() {
            let mut node = &mut root;
            for segment in file.path.split('/') {
                node = node.children.entry(segment.to_owned()).or_default();
            }
            node.file_index = Some(file_index);
        }
        let mut rows = vec![];
        flatten_tree(&root, "", 0, &self.collapsed_dirs, &mut rows);
        rows
    }

    fn active_file_index(&self) -> Option<usize> {
        self.file_tree_rows()
            .get(self.selected_file)
            .and_then(|row| row.file_index)
    }

    fn select_file(&mut self, file_index: usize) -> Result<()> {
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

    fn toggle_selected_directory(&mut self) {
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

    fn rebuild_diff(&mut self) -> Result<()> {
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

    fn set_diff_mode(&mut self, mode: DiffMode) -> Result<()> {
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

    fn active_rows(&self) -> &[DiffRow] {
        if self.final_view {
            &self.final_rows
        } else {
            &self.diff_rows
        }
    }

    fn displayed_indices(&self) -> Vec<usize> {
        displayed_diff_indices(
            self.active_rows(),
            self.final_view || self.show_unchanged,
            self.diff_mode,
        )
    }

    fn rendered_lines(&self) -> Vec<RenderedDiffLine> {
        rendered_diff_lines(
            self.active_rows(),
            self.final_view || self.show_unchanged,
            self.diff_mode,
        )
    }

    fn toggle_final_view(&mut self, viewport_height: u16) {
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

    fn save_filters(&self) -> Result<()> {
        save_json(&self.filters_path(), &self.filters)
    }
    fn save_comments(&self) -> Result<()> {
        save_json(&self.local_comments_path(), &self.local_comments)
    }
    fn save_settings(&self) -> Result<()> {
        save_json(
            &self.settings_path(),
            &ProjectSettings::new(self.diff_mode, self.show_unchanged, self.divider),
        )
    }

    fn add_comment(&mut self, summary: String) -> Result<()> {
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

    fn add_filter(&mut self, glob: String) -> Result<()> {
        Glob::new(&glob).with_context(|| format!("invalid glob: {glob}"))?;
        if !self.filters.patterns.contains(&glob) {
            self.filters.patterns.push(glob);
            self.save_filters()?;
        }
        self.refresh()?;
        self.message = "filter saved".into();
        Ok(())
    }

    fn copy_comment(&mut self) -> Result<()> {
        let comments = self.all_comments();
        let comment = comments
            .get(self.selected_comment)
            .context("choose a comment first")?;
        let payload = serde_json::to_string_pretty(comment)?;
        Clipboard::new()?.set_text(payload)?;
        self.message = "comment JSON copied to clipboard".into();
        Ok(())
    }

    fn delete_selected_comment(&mut self) -> Result<()> {
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

    fn delete_all_comments(&mut self) -> Result<()> {
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

fn remove_local_comment(store: &mut CommentStore, selected: usize) -> Option<ReviewComment> {
    (selected < store.comments.len()).then(|| store.comments.remove(selected))
}

fn flatten_tree(
    node: &TreeNode,
    prefix: &str,
    depth: usize,
    collapsed_dirs: &BTreeSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    for (name, child) in &node.children {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let is_directory = !child.children.is_empty();
        let expanded = is_directory && !collapsed_dirs.contains(&path);
        rows.push(TreeRow {
            path: path.clone(),
            depth,
            file_index: child.file_index,
            expanded,
        });
        if is_directory && expanded {
            flatten_tree(child, &path, depth + 1, collapsed_dirs, rows);
        }
    }
}

fn adjacent_file_index(
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

fn max_diff_scroll(content_height: usize, viewport_height: u16) -> u16 {
    content_height
        .saturating_sub(viewport_height as usize)
        .min(u16::MAX as usize) as u16
}

fn clamped_diff_scroll(scroll: u16, content_height: usize, viewport_height: u16) -> u16 {
    scroll.min(max_diff_scroll(content_height, viewport_height))
}

fn diff_content_width(app: &App) -> usize {
    if app.diff_mode == DiffMode::Unified {
        return app
            .displayed_indices()
            .iter()
            .flat_map(|index| unified_lines(*index, &app.active_rows()[*index], app.selected_row))
            .map(|line| line.width())
            .max()
            .unwrap_or(0);
    }

    app.rendered_lines()
        .iter()
        .map(|line| match line {
            RenderedDiffLine::Row(index) => {
                let row = &app.diff_rows[*index];
                let old = side_line(
                    *index,
                    row.old_line,
                    &row.old_text,
                    &row.old_spans,
                    row.old_changed,
                    true,
                    app.selected_row,
                )
                .width();
                let new = side_line(
                    *index,
                    row.new_line,
                    &row.new_text,
                    &row.new_spans,
                    row.new_changed,
                    false,
                    app.selected_row,
                )
                .width();
                old.max(new)
            }
            RenderedDiffLine::Separator => split_separator_line().width(),
        })
        .max()
        .unwrap_or(0)
}

fn diff_inner_width(app: &App, terminal_width: u16) -> u16 {
    let panel_width = match app.maximized_panel {
        Some(Focus::Right) => terminal_width,
        Some(_) => 0,
        None => terminal_width.saturating_sub(constrained_divider(app.divider, terminal_width)),
    };
    panel_width.saturating_sub(2)
}

fn diff_horizontal_viewport_width(app: &App, terminal_width: u16) -> u16 {
    let inner_width = diff_inner_width(app, terminal_width);
    if app.diff_mode == DiffMode::Unified {
        return inner_width;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(0, 0, inner_width, 1));
    columns[0].width.min(columns[1].width.saturating_sub(1))
}

fn max_diff_horizontal_scroll(app: &App, terminal_width: u16) -> u16 {
    diff_content_width(app)
        .saturating_sub(diff_horizontal_viewport_width(app, terminal_width) as usize)
        .min(u16::MAX as usize) as u16
}

fn move_diff_horizontally(app: &mut App, right: bool, terminal_width: u16) {
    const STEP: u16 = 4;
    let max_scroll = max_diff_horizontal_scroll(app, terminal_width);
    let current = app.diff_horizontal_scroll.min(max_scroll);
    if right {
        app.diff_horizontal_scroll = current.saturating_add(STEP).min(max_scroll);
    } else {
        app.diff_horizontal_scroll = current.saturating_sub(STEP);
    }
}

fn diff_viewport_height(terminal_height: u16) -> u16 {
    terminal_height.saturating_sub(3)
}

fn active_diff_viewport_height(app: &App, terminal_width: u16, terminal_height: u16) -> u16 {
    diff_viewport_height(terminal_height).saturating_sub(u16::from(
        diff_content_width(app) > diff_horizontal_viewport_width(app, terminal_width) as usize,
    ))
}

fn nearest_source_row(rows: &[DiffRow], line: Option<u32>) -> usize {
    rows.iter()
        .enumerate()
        .min_by_key(|(_, row)| {
            row.new_line
                .map_or(u32::MAX, |n| n.abs_diff(line.unwrap_or(1)))
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn move_diff_selection(app: &mut App, down: bool, viewport_height: u16) {
    let visible = app.displayed_indices();
    if visible.is_empty() {
        return;
    }
    let current = visible
        .iter()
        .position(|index| *index == app.selected_row)
        .unwrap_or(0);
    let next = if down {
        (current + 1).min(visible.len() - 1)
    } else {
        current.saturating_sub(1)
    };
    app.selected_row = visible[next];
    scroll_diff_selection_into_view(app, viewport_height);
}

fn scroll_diff_selection_into_view(app: &mut App, viewport_height: u16) {
    let rendered = app.rendered_lines();
    let rendered_start = rendered
        .iter()
        .position(|line| line.row_index() == Some(app.selected_row))
        .unwrap_or(0);
    let rendered_end = rendered
        .iter()
        .rposition(|line| line.row_index() == Some(app.selected_row))
        .unwrap_or(rendered_start);
    let mut scroll = clamped_diff_scroll(app.diff_scroll, rendered.len(), viewport_height) as usize;
    if rendered_start < scroll {
        scroll = rendered_start;
    } else if rendered_end >= scroll + viewport_height as usize {
        scroll = rendered_end
            .saturating_add(1)
            .saturating_sub(viewport_height as usize);
    }
    app.diff_scroll = clamped_diff_scroll(
        scroll.min(u16::MAX as usize) as u16,
        rendered.len(),
        viewport_height,
    );
}

fn move_change_selection(app: &mut App, forward: bool, viewport_height: u16) -> Result<()> {
    app.right_tab = RightTab::Diff;
    app.focus_panel(Focus::Right);

    // Change navigation returns to the diff so deletions have a visible target.
    if app.final_view {
        app.toggle_final_view(viewport_height);
    }

    if let Some(target) = adjacent_changed_row(&app.diff_rows, app.selected_row, forward) {
        app.selected_row = target;
        scroll_diff_selection_into_view(app, viewport_height);
        return Ok(());
    }

    let current_file = app.active_file_index();
    if let Some(file_index) = adjacent_file_index(&app.files, current_file, forward) {
        app.select_file(file_index)?;
        if !forward && let Some(last_change) = changed_row_indices(&app.diff_rows).last().copied() {
            app.selected_row = last_change;
            scroll_diff_selection_into_view(app, viewport_height);
        }
    } else {
        app.message = if forward {
            "already at the last changed file"
        } else {
            "already at the first changed file"
        }
        .into();
    }
    Ok(())
}

fn move_file_selection(app: &mut App, forward: bool) -> Result<()> {
    let current_file = app.active_file_index();
    if let Some(file_index) = adjacent_file_index(&app.files, current_file, forward) {
        app.select_file(file_index)?;
    } else {
        app.message = if forward {
            "already at the last changed file"
        } else {
            "already at the first changed file"
        }
        .into();
    }
    Ok(())
}

fn toggle_unchanged(app: &mut App) {
    app.show_unchanged = !app.show_unchanged;
    let displayed = app.displayed_indices();
    if !app.show_unchanged && !displayed.contains(&app.selected_row) {
        app.selected_row = app
            .diff_rows
            .iter()
            .position(DiffRow::is_changed)
            .unwrap_or(0);
    }
    app.diff_scroll = 0;
    app.message = if app.show_unchanged {
        "showing unchanged code"
    } else {
        "hiding unchanged code"
    }
    .into();
}

fn read_json<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    Ok(serde_json::from_slice(
        &fs::read(path).with_context(|| format!("reading {}", path.display()))?,
    )?)
}
fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::create_dir_all(path.parent().context("metadata path missing parent")?)?;
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}
fn compile_filters(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid saved filter: {p}"))?);
    }
    Ok(builder.build()?)
}

fn partition_filtered_files(
    files: Vec<FileItem>,
    filter: &GlobSet,
) -> (Vec<FileItem>, Vec<FileItem>) {
    files
        .into_iter()
        .partition(|file| filter.is_match(&file.path))
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
fn find_repo(dir: &Path) -> Result<PathBuf> {
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
fn changed_files(repo: &Path) -> Result<Vec<FileItem>> {
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
fn git_show_head_file(repo: &Path, path: &str) -> Result<String> {
    let spec = format!("HEAD:{path}");
    let output = git(repo, &["show", &spec])?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}
fn remote_status(repo: &Path) -> RemoteStatus {
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

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);
    match app.maximized_panel {
        Some(Focus::Files) => draw_files(frame, app, vertical[0]),
        Some(Focus::Filters) => draw_filters(frame, app, vertical[0]),
        Some(Focus::Right) => draw_right(frame, app, vertical[0]),
        None => {
            let width = constrained_divider(app.divider, vertical[0].width);
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(width), Constraint::Min(28)])
                .split(vertical[0]);
            draw_left(frame, app, panes[0]);
            draw_right(frame, app, panes[1]);
        }
    }
    draw_footer(frame, app, vertical[1]);
    if let Some(input) = &app.input {
        if input.kind == InputKind::FileSearch {
            draw_file_search(frame, app, input, area);
        } else {
            draw_input(frame, input, area);
        }
    }
    if app.confirmation == Some(ConfirmationKind::DeleteAllComments) {
        draw_delete_all_confirmation(frame, area);
    }
    if app.show_help {
        draw_help(frame, area);
    }
}

fn constrained_divider(requested: u16, terminal_width: u16) -> u16 {
    // Terminal multiplexers can briefly report a 0×0 size during startup or a
    // resize. Keep layout arithmetic total so the UI never panics there.
    if terminal_width < 52 {
        return terminal_width.saturating_div(2).max(1);
    }
    requested.clamp(24, terminal_width - 28)
}

fn resize_focused_panel(divider: &mut u16, focus: Focus, wider: bool, terminal_width: u16) {
    const STEP: u16 = 4;
    let focused_on_left = focus != Focus::Right;
    let grow_left = focused_on_left == wider;
    let requested = if grow_left {
        divider.saturating_add(STEP)
    } else {
        divider.saturating_sub(STEP)
    };
    *divider = constrained_divider(requested, terminal_width);
}

fn draw_left(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let panels = left_panel_areas(area);
    draw_files(frame, app, panels[0]);
    draw_filters(frame, app, panels[1]);
}

fn left_panel_areas(area: Rect) -> [Rect; 2] {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(area);
    [panels[0], panels[1]]
}

fn draw_files(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let tree_rows = app.file_tree_rows();
    let items: Vec<_> = tree_rows
        .iter()
        .map(|row| {
            let name = row.path.rsplit('/').next().unwrap_or(&row.path);
            let indent = "  ".repeat(row.depth);
            if let Some(file_index) = row.file_index {
                let file = &app.files[file_index];
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{indent}  ")),
                    Span::styled(
                        format!("{} ", file.status.trim()),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(name.to_owned()),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{indent}{} ", if row.expanded { "▼" } else { "▶" }),
                        accent_style(),
                    ),
                    Span::styled(
                        name.to_owned(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
        })
        .collect();
    let list = List::new(items)
        .block(single_panel_block(
            "[1]",
            "Files",
            app.focus == Focus::Files,
        ))
        .highlight_style(selected_row_style());
    let mut state = ratatui::widgets::ListState::default();
    state.select((!tree_rows.is_empty()).then_some(app.selected_file));
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        tree_rows.len(),
        area.height.saturating_sub(2) as usize,
        state.offset(),
    );
}

fn draw_filters(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let (items, selected) = match app.filter_tab {
        FilterTab::Filters => (
            app.filters
                .patterns
                .iter()
                .map(|pattern| ListItem::new(format!("  {pattern}")))
                .collect::<Vec<_>>(),
            (!app.filters.patterns.is_empty()).then_some(app.selected_filter),
        ),
        FilterTab::Ignored => (
            app.ignored_files
                .iter()
                .map(|file| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", file.status.trim()),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(file.path.clone()),
                    ]))
                })
                .collect::<Vec<_>>(),
            (!app.ignored_files.is_empty()).then_some(app.selected_ignored),
        ),
    };
    let list = List::new(items)
        .block(panel_block(
            "[3]",
            "Filters",
            app.filter_tab == FilterTab::Filters,
            "Ignored",
            app.filter_tab == FilterTab::Ignored,
            app.focus == Focus::Filters,
        ))
        .highlight_style(selected_row_style())
        .highlight_symbol("›");
    let mut state = ratatui::widgets::ListState::default();
    state.select(selected);
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        match app.filter_tab {
            FilterTab::Filters => app.filters.patterns.len(),
            FilterTab::Ignored => app.ignored_files.len(),
        },
        area.height.saturating_sub(2) as usize,
        state.offset(),
    );
}

fn draw_right(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    match app.right_tab {
        RightTab::Diff => draw_diff(frame, app, area),
        RightTab::Comments => draw_comments(frame, app, area),
    }
}

fn accent_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

fn selected_row_style() -> Style {
    accent_style().bg(SELECTION_BACKGROUND)
}

fn rounded_block<'a>() -> Block<'a> {
    Block::default().border_type(BorderType::Rounded)
}

fn scrollbar_position(content_length: usize, viewport_length: usize, offset: usize) -> usize {
    let max_offset = content_length.saturating_sub(viewport_length);
    if max_offset == 0 {
        return 0;
    }
    offset
        .min(max_offset)
        .saturating_mul(content_length.saturating_sub(1))
        / max_offset
}

fn render_vertical_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    offset: usize,
) {
    if content_length <= viewport_length
        || viewport_length == 0
        || area.width == 0
        || area.height <= 2
    {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_symbol("┃")
        .thumb_style(Style::default().fg(Color::Gray));
    let mut state = ScrollbarState::new(content_length)
        .position(scrollbar_position(content_length, viewport_length, offset))
        .viewport_content_length(viewport_length);
    frame.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}

fn render_horizontal_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    offset: usize,
) {
    if content_length <= viewport_length || viewport_length == 0 || area.width == 0 {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("─"))
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_symbol("━")
        .thumb_style(accent_style());
    let mut state = ScrollbarState::new(content_length)
        .position(scrollbar_position(content_length, viewport_length, offset))
        .viewport_content_length(viewport_length);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn single_panel_block(panel: &'static str, title: &'static str, focused: bool) -> Block<'static> {
    let muted = Style::default().fg(Color::DarkGray);
    rounded_block()
        .borders(Borders::ALL)
        .border_style(if focused { accent_style() } else { muted })
        .title(Line::from(vec![
            Span::styled(
                format!(" {panel} "),
                if focused { accent_style() } else { muted },
            ),
            Span::styled(format!("{title} "), accent_style()),
        ]))
}

fn panel_block(
    panel: &'static str,
    first: &'static str,
    first_active: bool,
    second: &'static str,
    second_active: bool,
    focused: bool,
) -> Block<'static> {
    let panel_style = if focused {
        accent_style()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let muted_style = Style::default().fg(Color::DarkGray);
    rounded_block()
        .borders(Borders::ALL)
        .border_style(if focused { accent_style() } else { muted_style })
        .title(Line::from(vec![
            Span::styled(format!(" {panel} "), panel_style),
            Span::styled(
                first,
                if first_active {
                    accent_style()
                } else {
                    muted_style
                },
            ),
            Span::styled(" - ", muted_style),
            Span::styled(
                format!("{second} "),
                if second_active {
                    accent_style()
                } else {
                    muted_style
                },
            ),
        ]))
}

fn right_panel_block(app: &App, changes_active: bool, comments_active: bool) -> Block<'static> {
    let path = app.active_path().unwrap_or("No changed file");
    let mut block = panel_block(
        "[2]",
        "Changes",
        changes_active,
        "Comments",
        comments_active,
        app.focus == Focus::Right,
    );
    if app.final_view && changes_active {
        block = block.title_bottom(Line::from(Span::styled(
            " Final · [h] show diff ",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(title) = right_panel_path_title(path, comments_active) {
        block.title(title)
    } else {
        block
    }
}

fn right_panel_path_title(path: &str, comments_active: bool) -> Option<Line<'static>> {
    (!comments_active).then(|| path_title(path).alignment(Alignment::Right))
}

fn path_title(path: &str) -> Line<'static> {
    let (directory, filename) = path
        .rsplit_once('/')
        .map_or((String::new(), path.to_owned()), |(directory, filename)| {
            (format!("{directory}/"), filename.to_owned())
        });

    Line::from(vec![
        Span::styled(
            format!(" {directory}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{filename} "), Style::default().fg(Color::Gray)),
    ])
}

fn draw_diff(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let outer = right_panel_block(app, true, false);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let diff_area = inner;
    let visible = app.displayed_indices();
    if visible.is_empty() {
        let message = if app.final_view {
            "Final version is empty (file empty or deleted).".to_owned()
        } else if !app.diff_has_syntactic_changes && !app.diff_language.is_empty() {
            format!("No syntactic changes ({})", app.diff_language)
        } else {
            "No changed lines to display.".to_owned()
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(Color::DarkGray)),
            diff_area,
        );
        return;
    }
    let (content_length, scroll, horizontal_content, horizontal_viewport, horizontal_scroll) =
        if app.diff_mode == DiffMode::Unified {
            let mut lines: Vec<_> = visible
                .iter()
                .flat_map(|index| {
                    unified_lines(*index, &app.active_rows()[*index], app.selected_row)
                })
                .collect();
            let horizontal_content = lines.iter().map(Line::width).max().unwrap_or(0);
            let horizontal_viewport = diff_area.width as usize;
            let horizontal_scroll = app.diff_horizontal_scroll.min(
                horizontal_content
                    .saturating_sub(horizontal_viewport)
                    .min(u16::MAX as usize) as u16,
            );
            let has_horizontal_scroll = horizontal_content > horizontal_viewport;
            let content_area = Rect {
                height: diff_area
                    .height
                    .saturating_sub(u16::from(has_horizontal_scroll)),
                ..diff_area
            };
            let padded_width = horizontal_content
                .max(horizontal_viewport)
                .min(u16::MAX as usize) as u16;
            lines = lines
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            let scroll = clamped_diff_scroll(app.diff_scroll, lines.len(), content_area.height);
            let content_length = lines.len();
            frame.render_widget(
                Paragraph::new(lines).scroll((scroll, horizontal_scroll)),
                content_area,
            );
            (
                content_length,
                scroll as usize,
                horizontal_content,
                horizontal_viewport,
                horizontal_scroll as usize,
            )
        } else {
            let rendered = app.rendered_lines();
            let initial_columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(diff_area);
            let mut old: Vec<_> = rendered
                .iter()
                .map(|line| match line {
                    RenderedDiffLine::Row(index) => {
                        let row = &app.diff_rows[*index];
                        side_line(
                            *index,
                            row.old_line,
                            &row.old_text,
                            &row.old_spans,
                            row.old_changed,
                            true,
                            app.selected_row,
                        )
                    }
                    RenderedDiffLine::Separator => split_separator_line(),
                })
                .collect();
            let mut new: Vec<_> = rendered
                .iter()
                .map(|line| match line {
                    RenderedDiffLine::Row(index) => {
                        let row = &app.diff_rows[*index];
                        side_line(
                            *index,
                            row.new_line,
                            &row.new_text,
                            &row.new_spans,
                            row.new_changed,
                            false,
                            app.selected_row,
                        )
                    }
                    RenderedDiffLine::Separator => split_separator_line(),
                })
                .collect();
            let horizontal_content = old.iter().chain(&new).map(Line::width).max().unwrap_or(0);
            let horizontal_viewport = initial_columns[0]
                .width
                .min(initial_columns[1].width.saturating_sub(1))
                as usize;
            let horizontal_scroll = app.diff_horizontal_scroll.min(
                horizontal_content
                    .saturating_sub(horizontal_viewport)
                    .min(u16::MAX as usize) as u16,
            );
            let has_horizontal_scroll = horizontal_content > horizontal_viewport;
            let content_area = Rect {
                height: diff_area
                    .height
                    .saturating_sub(u16::from(has_horizontal_scroll)),
                ..diff_area
            };
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(content_area);
            let padded_width = horizontal_content
                .max(horizontal_viewport)
                .min(u16::MAX as usize) as u16;
            old = old
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            new = new
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            let scroll = clamped_diff_scroll(app.diff_scroll, rendered.len(), content_area.height);
            frame.render_widget(
                Paragraph::new(old).scroll((scroll, horizontal_scroll)),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new(new)
                    .scroll((scroll, horizontal_scroll))
                    .block(
                        rounded_block()
                            .borders(Borders::LEFT)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    ),
                columns[1],
            );
            (
                rendered.len(),
                scroll as usize,
                horizontal_content,
                horizontal_viewport,
                horizontal_scroll as usize,
            )
        };
    render_vertical_scrollbar(
        frame,
        area,
        content_length,
        diff_area
            .height
            .saturating_sub(u16::from(horizontal_content > horizontal_viewport)) as usize,
        scroll,
    );
    render_horizontal_scrollbar(
        frame,
        diff_area,
        horizontal_content,
        horizontal_viewport,
        horizontal_scroll,
    );
}
fn draw_comments(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let comments = app.all_comments();
    let items: Vec<_> = comments
        .iter()
        .map(|c| {
            let anchor = c
                .new_line
                .map(|l| format!("new:{l}"))
                .or_else(|| c.old_line.map(|l| format!("old:{l}")))
                .unwrap_or_else(|| "hunk".into());
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{}  {}", c.file_path, anchor),
                    accent_style(),
                )),
                Line::from(c.summary.clone()),
                Line::from(Span::styled(
                    format!(
                        "{} · {}",
                        c.source,
                        c.author.clone().unwrap_or_else(|| "anonymous".into())
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(right_panel_block(app, false, true))
        .highlight_style(selected_row_style());
    let mut state = ratatui::widgets::ListState::default();
    state.select((!comments.is_empty()).then_some(app.selected_comment));
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        comments.len(),
        area.height.saturating_sub(2).saturating_div(3).max(1) as usize,
        state.offset(),
    );
}

fn draw_footer(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let status = remote_status_line(app);
    let status_width = (status.width() as u16)
        .saturating_add(1)
        .min(area.width.saturating_sub(3));
    let pieces = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(status_width)])
        .split(area);
    let shortcuts = shortcut_line(app);
    let mut shortcuts_with_message = shortcuts.clone();
    shortcuts_with_message.spans.push(Span::styled(
        format!("  ·  {}", app.message),
        Style::default().fg(Color::DarkGray),
    ));
    let shortcuts = if shortcuts_with_message.width() <= pieces[0].width as usize {
        shortcuts_with_message
    } else if shortcuts.width() <= pieces[0].width as usize {
        shortcuts
    } else {
        Line::from(Span::styled(
            " ? ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    };
    frame.render_widget(Paragraph::new(shortcuts), pieces[0]);
    frame.render_widget(
        Paragraph::new(status).alignment(Alignment::Right),
        pieces[1],
    );
}

fn push_shortcut_display(
    spans: &mut Vec<Span<'static>>,
    display: impl Into<String>,
    label: impl Into<String>,
) {
    if !spans.is_empty() {
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled(
        display.into(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        format!(" {}", label.into()),
        Style::default().fg(Color::Gray),
    ));
}

fn push_shortcut(spans: &mut Vec<Span<'static>>, key: &'static str, label: impl Into<String>) {
    push_shortcut_display(spans, format!("[{key}]"), label);
}

fn shortcut_line(app: &App) -> Line<'static> {
    let mut spans = vec![];
    match (app.focus, app.right_tab, app.filter_tab) {
        (Focus::Files, _, _) => {
            push_shortcut(&mut spans, "↑/↓", "move");
            push_shortcut(&mut spans, "Enter", "open/fold");
        }
        (Focus::Filters, _, FilterTab::Filters) => {
            push_shortcut(&mut spans, "Tab", "next tab");
            push_shortcut(&mut spans, "a", "add");
            push_shortcut(&mut spans, "x", "remove");
        }
        (Focus::Filters, _, FilterTab::Ignored) => {
            push_shortcut(&mut spans, "Tab", "next tab");
            push_shortcut(&mut spans, "↑/↓", "move");
        }
        (Focus::Right, RightTab::Diff, _) => {
            push_shortcut(&mut spans, "Tab", "next tab");
            push_shortcut(&mut spans, "↑/↓", "line");
            push_shortcut(&mut spans, "←/→", "scroll");
            push_shortcut_display(&mut spans, "[ / ]", "change");
            push_shortcut_display(&mut spans, "{ / }", "file");
            push_shortcut(&mut spans, "c", "comment");
            if app.diff_mode == DiffMode::SideBySide {
                push_shortcut(&mut spans, "u", "unified");
            } else {
                push_shortcut(&mut spans, "s", "split");
                push_shortcut(
                    &mut spans,
                    "h",
                    if app.final_view {
                        "show diff"
                    } else {
                        "show final"
                    },
                );
            }
            if !app.final_view {
                push_shortcut(
                    &mut spans,
                    "i",
                    if app.show_unchanged {
                        "hide common"
                    } else {
                        "show common"
                    },
                );
            }
        }
        (Focus::Right, RightTab::Comments, _) => {
            push_shortcut(&mut spans, "Tab", "next tab");
            push_shortcut(&mut spans, "↑/↓", "move");
            push_shortcut(&mut spans, "y", "copy JSON");
            push_shortcut(&mut spans, "d", "delete");
            push_shortcut(&mut spans, "D", "delete all");
        }
    }
    push_shortcut(&mut spans, "</>", "width");
    push_shortcut(&mut spans, "/", "find file");
    push_shortcut(&mut spans, "?", "help");
    Line::from(spans)
}

fn remote_status_line(app: &App) -> Line<'static> {
    let project = app
        .repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("git");
    let white = Style::default().fg(Color::White);
    let yellow = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(project.to_owned(), white.add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.remote.branch.clone(), white),
    ];
    match (app.remote.behind, app.remote.ahead) {
        (Some(behind), Some(ahead)) => {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("↓", yellow));
            spans.push(Span::styled(behind.to_string(), white));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("↑", yellow));
            spans.push(Span::styled(ahead.to_string(), white));
        }
        _ => {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("◆", yellow));
            spans.push(Span::styled(" no upstream", white));
        }
    }
    Line::from(spans)
}

fn help_binding(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>16}  "), Style::default().fg(Color::Cyan)),
        Span::styled(description, Style::default().fg(Color::Gray)),
    ])
}

fn help_heading(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled("──────── ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ────────", Style::default().fg(Color::DarkGray)),
    ])
    .alignment(Alignment::Center)
}

fn draw_help(frame: &mut ratatui::Frame, area: Rect) {
    let width = 76.min(area.width.saturating_sub(2)).max(1);
    let height = 34.min(area.height.saturating_sub(1)).max(1);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let lines = vec![
        help_heading("GLOBAL"),
        help_binding("1 / 2 / 3", "focus panel; repeat to maximize / restore"),
        help_binding("Tab", "cycle tabs within the focused panel"),
        help_binding("f / l", "focus files / changes"),
        help_binding("< / >", "narrow / widen focused panel"),
        help_binding("[ / ]", "previous / next changed row or file"),
        help_binding("{ / }", "previous / next changed file"),
        help_binding("/", "find a changed file"),
        help_binding("r", "refresh"),
        help_binding("? / Esc", "close help"),
        help_binding("q", "quit"),
        Line::default(),
        help_heading("[1] FILES"),
        help_binding("↑ / ↓", "move selection"),
        help_binding("Enter", "open file or fold directory"),
        Line::default(),
        help_heading("[2] CHANGES"),
        help_binding("↑ / ↓", "move through changed lines"),
        help_binding("← / →", "scroll code horizontally"),
        help_binding("u / s · h", "unified / split; h toggles final in unified"),
        help_binding("i", "show or hide common lines"),
        help_binding("c", "add review comment"),
        Line::default(),
        help_heading("[2] COMMENTS"),
        help_binding("Tab", "cycle changes / comments"),
        help_binding("y", "copy selected comment JSON"),
        help_binding("d / D", "delete selected / all local comments"),
        Line::default(),
        help_heading("[3] FILTERS / IGNORED"),
        help_binding("Tab", "cycle filters / ignored"),
        help_binding("a / x", "add / remove filter"),
        help_binding("↑ / ↓", "move selection"),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Line::from(Span::styled(
                    " Keybindings ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )))
                .title_bottom(
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("Esc", Style::default().fg(Color::Yellow)),
                        Span::raw(" Close "),
                    ])
                    .alignment(Alignment::Center),
                ),
        ),
        popup,
    );
}
fn draw_input(frame: &mut ratatui::Frame, input: &Input, area: Rect) {
    let popup = centered_popup(area, area.width.saturating_mul(3) / 4, 5);
    frame.render_widget(Clear, popup);
    let label = if input.kind == InputKind::Comment {
        "Comment (Enter saves, Esc cancels)"
    } else {
        "Exclude glob (Enter saves, Esc cancels)"
    };
    frame.render_widget(
        Paragraph::new(input.value.as_str()).block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(accent_style())
                .title(label),
        ),
        popup,
    );
}

fn draw_delete_all_confirmation(frame: &mut ratatui::Frame, area: Rect) {
    let popup = centered_popup(area, 52, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::default(),
            Line::from("Are you sure you want to delete all comments?"),
        ])
        .alignment(Alignment::Center)
        .block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Line::from(Span::styled(
                    " Confirmation Required ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )))
                .title_bottom(
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("Enter", accent_style()),
                        Span::raw(" Confirm  ·  "),
                        Span::styled("Esc", Style::default().fg(Color::Yellow)),
                        Span::raw(" Cancel "),
                    ])
                    .alignment(Alignment::Center),
                ),
        ),
        popup,
    );
}

fn centered_popup(area: Rect, requested_width: u16, requested_height: u16) -> Rect {
    let width = requested_width.min(area.width.saturating_sub(2)).max(1);
    let height = requested_height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn draw_file_search(frame: &mut ratatui::Frame, app: &App, input: &Input, area: Rect) {
    let matches = fuzzy_file_indices(&app.files, &input.value);
    let height = (matches.len().min(11) as u16).saturating_add(3).max(5);
    let popup = centered_popup(area, area.width.saturating_mul(3) / 4, height);
    frame.render_widget(Clear, popup);

    let block = rounded_block()
        .borders(Borders::ALL)
        .border_style(accent_style())
        .title(Line::from(Span::styled(
            " Find file · Enter opens · Esc cancels ",
            accent_style(),
        )));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height == 0 {
        return;
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/ ", accent_style()),
            Span::raw(input.value.clone()),
        ])),
        sections[0],
    );
    let items = if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No matching files",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        matches
            .iter()
            .map(|index| ListItem::new(format!("  {}", app.files[*index].path)))
            .collect::<Vec<_>>()
    };
    let mut state = ratatui::widgets::ListState::default();
    state.select((!matches.is_empty()).then(|| input.selected.min(matches.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(selected_row_style())
            .highlight_symbol("›"),
        sections[1],
        &mut state,
    );
    render_vertical_scrollbar(
        frame,
        popup,
        matches.len(),
        sections[1].height as usize,
        state.offset(),
    );
}

fn move_selection(current: &mut usize, len: usize, down: bool) {
    if len == 0 {
        return;
    }
    *current = if down {
        (*current + 1).min(len - 1)
    } else {
        current.saturating_sub(1)
    };
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    let mut search_from = 0;
    let mut previous = None;
    let mut score = 0;
    for needle in query.to_lowercase().chars() {
        let (offset, _) = candidate[search_from..]
            .char_indices()
            .find(|(_, character)| *character == needle)?;
        let index = search_from + offset;
        score -= index as i64;
        if previous.is_some_and(|previous| previous + 1 == index) {
            score += 12;
        }
        if index == 0
            || candidate[..index]
                .chars()
                .next_back()
                .is_some_and(|character| matches!(character, '/' | '_' | '-' | '.'))
        {
            score += 8;
        }
        previous = Some(index);
        search_from = index + needle.len_utf8();
    }
    if candidate.contains(&query.to_lowercase()) {
        score += 24;
    }
    Some(score)
}

fn fuzzy_file_indices(files: &[FileItem], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..files.len()).collect();
    }
    let mut matches = files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| fuzzy_score(&file.path, query).map(|score| (index, score)))
        .collect::<Vec<_>>();
    matches.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| {
                files[*left_index]
                    .path
                    .len()
                    .cmp(&files[*right_index].path.len())
            })
            .then_with(|| files[*left_index].path.cmp(&files[*right_index].path))
    });
    matches.into_iter().map(|(index, _)| index).collect()
}

fn handle_file_search_key(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.input = None,
        KeyCode::Enter => {
            let file_index = app.input.as_ref().and_then(|input| {
                fuzzy_file_indices(&app.files, &input.value)
                    .get(input.selected)
                    .copied()
            });
            if let Some(file_index) = file_index {
                app.input = None;
                app.select_file(file_index)?;
            }
        }
        KeyCode::Down | KeyCode::Up => {
            let query = app
                .input
                .as_ref()
                .map(|input| input.value.as_str())
                .unwrap_or_default();
            let match_count = fuzzy_file_indices(&app.files, query).len();
            if let Some(input) = &mut app.input {
                move_selection(&mut input.selected, match_count, code == KeyCode::Down);
            }
        }
        KeyCode::Backspace => {
            if let Some(input) = &mut app.input {
                input.value.pop();
                input.selected = 0;
            }
        }
        KeyCode::Char(character) => {
            if let Some(input) = &mut app.input {
                input.value.push(character);
                input.selected = 0;
            }
        }
        _ => {}
    }
    Ok(())
}

fn activate_numbered_panel(panel: u8, focus: &mut Focus, maximized_panel: &mut Option<Focus>) {
    let target = match panel {
        1 => Focus::Files,
        2 => Focus::Right,
        3 => Focus::Filters,
        _ => return,
    };
    if *focus != target {
        *focus = target;
        *maximized_panel = None;
    } else if *maximized_panel == Some(target) {
        *maximized_panel = None;
    } else {
        *maximized_panel = Some(target);
    }
}

fn cycle_focused_panel_tab(focus: Focus, right_tab: &mut RightTab, filter_tab: &mut FilterTab) {
    match focus {
        Focus::Right => {
            *right_tab = match *right_tab {
                RightTab::Diff => RightTab::Comments,
                RightTab::Comments => RightTab::Diff,
            }
        }
        Focus::Filters => {
            *filter_tab = match *filter_tab {
                FilterTab::Filters => FilterTab::Ignored,
                FilterTab::Ignored => FilterTab::Filters,
            }
        }
        Focus::Files => {}
    }
}

fn handle_key(
    app: &mut App,
    code: KeyCode,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<bool> {
    if app.show_help {
        if matches!(code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')) {
            app.show_help = false;
        }
        return Ok(false);
    }
    if app.confirmation == Some(ConfirmationKind::DeleteAllComments) {
        match code {
            KeyCode::Enter => {
                app.confirmation = None;
                app.delete_all_comments()?;
            }
            KeyCode::Esc => {
                app.confirmation = None;
                app.message = "delete cancelled".into();
            }
            _ => {}
        }
        return Ok(false);
    }
    if app
        .input
        .as_ref()
        .is_some_and(|input| input.kind == InputKind::FileSearch)
    {
        handle_file_search_key(app, code)?;
        return Ok(false);
    }
    if let Some(input) = &mut app.input {
        match code {
            KeyCode::Esc => app.input = None,
            KeyCode::Enter => {
                let input = app.input.take().expect("input exists");
                if input.value.trim().is_empty() {
                    return Ok(false);
                }
                match input.kind {
                    InputKind::Comment => app.add_comment(input.value)?,
                    InputKind::Filter => app.add_filter(input.value)?,
                    InputKind::FileSearch => unreachable!("file search handled above"),
                };
            }
            KeyCode::Backspace => {
                input.value.pop();
            }
            KeyCode::Char(c) => input.value.push(c),
            _ => {}
        }
        return Ok(false);
    }
    match code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('/') => {
            app.input = Some(Input {
                kind: InputKind::FileSearch,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Tab => cycle_focused_panel_tab(app.focus, &mut app.right_tab, &mut app.filter_tab),
        KeyCode::Char('h')
            if app.right_tab == RightTab::Diff && app.diff_mode == DiffMode::Unified =>
        {
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            app.toggle_final_view(viewport);
        }
        KeyCode::Char('h') => app.focus_panel(Focus::Files),
        KeyCode::Char('l') => app.focus_panel(Focus::Right),
        KeyCode::Char('<') => {
            resize_focused_panel(&mut app.divider, app.focus, false, terminal_width)
        }
        KeyCode::Char('>') => {
            resize_focused_panel(&mut app.divider, app.focus, true, terminal_width)
        }
        KeyCode::Char('[') => {
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            move_change_selection(app, false, viewport)?
        }
        KeyCode::Char(']') => {
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            move_change_selection(app, true, viewport)?
        }
        KeyCode::Char('{') => move_file_selection(app, false)?,
        KeyCode::Char('}') => move_file_selection(app, true)?,
        KeyCode::Char('1') => activate_numbered_panel(1, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('2') => activate_numbered_panel(2, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('3') => activate_numbered_panel(3, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('f') => app.focus_panel(Focus::Files),
        KeyCode::Char('d') if app.focus == Focus::Right && app.right_tab == RightTab::Comments => {
            app.delete_selected_comment()?;
        }
        KeyCode::Char('D') if app.focus == Focus::Right && app.right_tab == RightTab::Comments => {
            app.confirmation = Some(ConfirmationKind::DeleteAllComments);
        }
        KeyCode::Char('u') => {
            app.set_diff_mode(DiffMode::Unified)?;
        }
        KeyCode::Char('s') => {
            app.set_diff_mode(DiffMode::SideBySide)?;
        }
        KeyCode::Char('i') if app.right_tab == RightTab::Diff && !app.final_view => {
            toggle_unchanged(app)
        }
        KeyCode::Char('r') => {
            app.refresh()?;
            app.message = "refreshed".into();
        }
        KeyCode::Char('c') if app.right_tab == RightTab::Diff => {
            app.input = Some(Input {
                kind: InputKind::Comment,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Char('a')
            if app.focus == Focus::Filters && app.filter_tab == FilterTab::Filters =>
        {
            app.input = Some(Input {
                kind: InputKind::Filter,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Char('x')
            if app.focus == Focus::Filters && app.filter_tab == FilterTab::Filters =>
        {
            if app.selected_filter < app.filters.patterns.len() {
                app.filters.patterns.remove(app.selected_filter);
                app.save_filters()?;
                app.refresh()?;
            }
        }
        KeyCode::Char('y') if app.right_tab == RightTab::Comments => app.copy_comment()?,
        KeyCode::Down => match (app.focus, app.right_tab) {
            (Focus::Files, _) => {
                let tree_len = app.file_tree_rows().len();
                move_selection(&mut app.selected_file, tree_len, true)
            }
            (Focus::Filters, _) => {
                if app.filter_tab == FilterTab::Filters {
                    move_selection(&mut app.selected_filter, app.filters.patterns.len(), true)
                } else {
                    move_selection(&mut app.selected_ignored, app.ignored_files.len(), true)
                }
            }
            (Focus::Right, RightTab::Diff) => {
                let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
                move_diff_selection(app, true, viewport);
            }
            (Focus::Right, RightTab::Comments) => {
                let comment_count = app.local_comments.comments.len() + app.agent_comments.len();
                move_selection(&mut app.selected_comment, comment_count, true)
            }
        },
        KeyCode::Up => match (app.focus, app.right_tab) {
            (Focus::Files, _) => {
                let tree_len = app.file_tree_rows().len();
                move_selection(&mut app.selected_file, tree_len, false)
            }
            (Focus::Filters, _) => {
                if app.filter_tab == FilterTab::Filters {
                    move_selection(&mut app.selected_filter, app.filters.patterns.len(), false)
                } else {
                    move_selection(&mut app.selected_ignored, app.ignored_files.len(), false)
                }
            }
            (Focus::Right, RightTab::Diff) => {
                let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
                move_diff_selection(app, false, viewport);
            }
            (Focus::Right, RightTab::Comments) => {
                let comment_count = app.local_comments.comments.len() + app.agent_comments.len();
                move_selection(&mut app.selected_comment, comment_count, false)
            }
        },
        KeyCode::Left if app.focus == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, false, terminal_width)
        }
        KeyCode::Right if app.focus == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, true, terminal_width)
        }
        KeyCode::Enter if app.focus == Focus::Files => {
            if app.active_file_index().is_some() {
                app.rebuild_diff()?;
                app.focus_panel(Focus::Right);
            } else {
                app.toggle_selected_directory();
            }
        }
        _ => {}
    }
    Ok(false)
}
fn handle_mouse(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<()> {
    if app.show_help || app.input.is_some() || app.confirmation.is_some() {
        return Ok(());
    }
    if let Some(panel) = app.maximized_panel {
        return handle_maximized_mouse(app, panel, mouse, terminal_width, terminal_height);
    }
    let divider = constrained_divider(app.divider, terminal_width);
    let left_panels = left_panel_areas(Rect::new(0, 0, divider, terminal_height.saturating_sub(1)));
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if mouse.column.abs_diff(divider) <= 1 => {
            app.dragging_divider = true
        }
        MouseEventKind::Drag(MouseButton::Left) if app.dragging_divider => {
            app.divider = constrained_divider(mouse.column, terminal_width)
        }
        MouseEventKind::Up(MouseButton::Left) => app.dragging_divider = false,
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row == 0 {
                if mouse.column < divider {
                    app.focus_panel(Focus::Files);
                } else {
                    app.focus_panel(Focus::Right);
                    let panel_column = mouse.column.saturating_sub(divider);
                    if panel_column < 16 {
                        app.right_tab = RightTab::Diff;
                    } else if panel_column < 27 {
                        app.right_tab = RightTab::Comments;
                    }
                }
            } else if mouse.column < divider {
                if mouse.row < left_panels[1].y {
                    app.focus_panel(Focus::Files);
                    let index =
                        mouse.row.saturating_sub(left_panels[0].y.saturating_add(1)) as usize;
                    let tree_rows = app.file_tree_rows();
                    app.selected_file = index.min(tree_rows.len().saturating_sub(1));
                    if tree_rows
                        .get(app.selected_file)
                        .and_then(|row| row.file_index)
                        .is_some()
                    {
                        app.rebuild_diff()?;
                    }
                } else {
                    app.focus_panel(Focus::Filters);
                    if mouse.row == left_panels[1].y {
                        let panel_column = mouse.column.saturating_sub(left_panels[1].x);
                        if panel_column < 16 {
                            app.filter_tab = FilterTab::Filters;
                        } else if panel_column < 27 {
                            app.filter_tab = FilterTab::Ignored;
                        }
                        return Ok(());
                    }
                    let index =
                        mouse.row.saturating_sub(left_panels[1].y.saturating_add(1)) as usize;
                    if app.filter_tab == FilterTab::Filters {
                        app.selected_filter =
                            index.min(app.filters.patterns.len().saturating_sub(1));
                    } else {
                        app.selected_ignored = index.min(app.ignored_files.len().saturating_sub(1));
                    }
                }
            } else {
                app.focus_panel(Focus::Right);
                if app.right_tab == RightTab::Diff {
                    let rendered = app.rendered_lines();
                    let scroll = clamped_diff_scroll(
                        app.diff_scroll,
                        rendered.len(),
                        active_diff_viewport_height(app, terminal_width, terminal_height),
                    );
                    let position = scroll as usize + mouse.row.saturating_sub(1) as usize;
                    if let Some(index) = rendered.get(position).and_then(|line| line.row_index()) {
                        app.selected_row = index;
                    }
                } else {
                    let index = mouse.row.saturating_sub(1) as usize;
                    app.selected_comment = index.min(app.all_comments().len().saturating_sub(1));
                }
            }
        }
        MouseEventKind::ScrollDown
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            let rendered = app.rendered_lines();
            app.diff_scroll = clamped_diff_scroll(
                app.diff_scroll.saturating_add(3),
                rendered.len(),
                active_diff_viewport_height(app, terminal_width, terminal_height),
            );
        }
        MouseEventKind::ScrollUp if app.right_tab == RightTab::Diff && mouse.column >= divider => {
            app.diff_scroll = app.diff_scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollRight
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            move_diff_horizontally(app, true, terminal_width);
        }
        MouseEventKind::ScrollLeft
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            move_diff_horizontally(app, false, terminal_width);
        }
        _ => {}
    }
    Ok(())
}

fn handle_maximized_mouse(
    app: &mut App,
    panel: Focus,
    mouse: crossterm::event::MouseEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => match panel {
            Focus::Files if mouse.row > 0 => {
                let index = mouse.row.saturating_sub(1) as usize;
                let tree_rows = app.file_tree_rows();
                app.selected_file = index.min(tree_rows.len().saturating_sub(1));
                if tree_rows
                    .get(app.selected_file)
                    .and_then(|row| row.file_index)
                    .is_some()
                {
                    app.rebuild_diff()?;
                }
            }
            Focus::Filters if mouse.row == 0 => {
                if mouse.column < 16 {
                    app.filter_tab = FilterTab::Filters;
                } else if mouse.column < 27 {
                    app.filter_tab = FilterTab::Ignored;
                }
            }
            Focus::Filters => {
                let index = mouse.row.saturating_sub(1) as usize;
                if app.filter_tab == FilterTab::Filters {
                    app.selected_filter = index.min(app.filters.patterns.len().saturating_sub(1));
                } else {
                    app.selected_ignored = index.min(app.ignored_files.len().saturating_sub(1));
                }
            }
            Focus::Right if mouse.row == 0 => {
                if mouse.column < 16 {
                    app.right_tab = RightTab::Diff;
                } else if mouse.column < 27 {
                    app.right_tab = RightTab::Comments;
                }
            }
            Focus::Right if app.right_tab == RightTab::Diff => {
                let rendered = app.rendered_lines();
                let scroll = clamped_diff_scroll(
                    app.diff_scroll,
                    rendered.len(),
                    active_diff_viewport_height(app, terminal_width, terminal_height),
                );
                let position = scroll as usize + mouse.row.saturating_sub(1) as usize;
                if let Some(index) = rendered.get(position).and_then(|line| line.row_index()) {
                    app.selected_row = index;
                }
            }
            Focus::Right => {
                let index = mouse.row.saturating_sub(1) as usize;
                app.selected_comment = index.min(app.all_comments().len().saturating_sub(1));
            }
            Focus::Files => {}
        },
        MouseEventKind::ScrollDown if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            let rendered = app.rendered_lines();
            app.diff_scroll = clamped_diff_scroll(
                app.diff_scroll.saturating_add(3),
                rendered.len(),
                active_diff_viewport_height(app, terminal_width, terminal_height),
            );
        }
        MouseEventKind::ScrollUp if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            app.diff_scroll = app.diff_scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollRight if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, true, terminal_width);
        }
        MouseEventKind::ScrollLeft if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, false, terminal_width);
        }
        _ => {}
    }
    Ok(())
}

fn run_tui(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(80))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        let size = terminal.size()?;
                        if handle_key(&mut app, key.code, size.width, size.height)? {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let size = terminal.size()?;
                        handle_mouse(&mut app, mouse, size.width, size.height)?
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            if app.input.is_none()
                && app.last_refresh.elapsed() >= Duration::from_millis(450)
                && let Err(error) = app.refresh()
            {
                app.message = error.to_string();
            }
        }
        Ok(())
    })();
    let settings_result = if result.is_ok() {
        app.save_settings()
    } else {
        Ok(())
    };
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result?;
    settings_result
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo = find_repo(&cli.dir)?;
    if !io::stdout().is_terminal() {
        bail!("Luminatti needs an interactive terminal");
    }
    run_tui(App::new(repo)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn test_diff_rows(before: &str, after: &str) -> Vec<DiffRow> {
        structural_diff_document(Path::new("example.txt"), before, after).rows
    }

    fn preview_app(before: &str, after: &str) -> App {
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
        }
    }

    #[test]
    fn final_toggle_shows_complete_source_and_restores_deleted_selection() {
        let mut app = preview_app("first\nremoved\nlast\n", "first\nlast\n");
        app.selected_row = app
            .diff_rows
            .iter()
            .position(|row| row.new_line.is_none())
            .unwrap();
        let deleted = app.selected_row;
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        assert!(app.final_view);
        assert_eq!(app.focus, Focus::Right);
        assert_eq!(app.displayed_indices(), vec![0, 1]);
        assert_eq!(
            app.active_rows()
                .iter()
                .map(|row| row.new_text.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "last"]
        );
        assert_eq!(app.selected_row, 1);
        handle_key(&mut app, KeyCode::Char('i'), 100, 20).unwrap();
        assert!(!app.show_unchanged);
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        assert!(!app.final_view);
        assert_eq!(app.selected_row, deleted);
        assert!(!app.show_unchanged);
    }

    #[test]
    fn final_toggle_is_available_when_another_panel_is_focused() {
        let mut app = preview_app("old\n", "new\n");

        app.focus = Focus::Files;
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        assert!(app.final_view);
        assert_eq!(app.focus, Focus::Files);

        app.focus = Focus::Filters;
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        assert!(!app.final_view);
        assert_eq!(app.focus, Focus::Filters);
    }

    #[test]
    fn final_navigation_and_mouse_use_physical_source_lines() {
        let mut app = preview_app("first\nold\nlast\n", "first\nnew\nlast\n");
        app.show_unchanged = true;
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        handle_key(&mut app, KeyCode::Down, 100, 20).unwrap();
        assert_eq!(app.active_rows()[app.selected_row].new_text, "new");
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 50,
                row: 3,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            100,
            20,
        )
        .unwrap();
        assert_eq!(app.active_rows()[app.selected_row].new_text, "last");
        handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
        assert_eq!(app.active_rows()[app.selected_row].new_line, Some(3));
        assert!(app.show_unchanged);
    }

    #[test]
    fn final_view_renders_neutral_numbered_source_and_handles_empty_files() {
        let mut app = preview_app("old\n", "new\n");
        app.toggle_final_view(5);
        let mut terminal = Terminal::new(TestBackend::new(60, 7)).unwrap();
        terminal
            .draw(|frame| draw_diff(frame, &app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let source: String = (1..20).map(|x| buffer[(x, 1)].symbol()).collect();
        assert!(source.starts_with("     1 new"));
        assert_eq!(buffer[(8, 1)].fg, Color::Gray);
        assert!(!buffer[(8, 1)].modifier.contains(Modifier::CROSSED_OUT));
        let mut empty = preview_app("removed\n", "");
        empty.toggle_final_view(5);
        assert!(empty.rendered_lines().is_empty());
        terminal
            .draw(|frame| draw_diff(frame, &empty, frame.area()))
            .unwrap();
        empty.toggle_final_view(5);
        assert!(!empty.rendered_lines().is_empty());
    }

    #[test]
    fn panel_tabs_render_in_the_top_border_with_purple_accents() {
        let backend = TestBackend::new(32, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                frame.render_widget(
                    panel_block("[1]", "Files", true, "Filters", false, true),
                    frame.area(),
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let top_border: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect();
        assert!(top_border.starts_with("╭ [1] Files - Filters "));
        assert_eq!(buffer[(0, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(2, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(6, 0)].fg, Color::Magenta);
        assert_eq!(buffer[(14, 0)].fg, Color::DarkGray);
    }

    #[test]
    fn keybindings_overlay_renders_as_a_grouped_command_palette() {
        let backend = TestBackend::new(80, 35);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| draw_help(frame, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let contents = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(contents.contains("Keybindings"));
        assert!(contents.contains("──────── GLOBAL ────────"));
        assert!(contents.contains("──────── [1] FILES ────────"));

        let popup = Rect::new(2, 0, 76, 34);
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(bottom_border.contains("Esc Close"));
    }

    #[test]
    fn path_title_mutes_the_directory_and_softens_the_filename() {
        let line = path_title("packages/business/src/NetsuiteConnector.ts");

        assert_eq!(line.spans[0].content, " packages/business/src/");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content, "NetsuiteConnector.ts ");
        assert_eq!(line.spans[1].style.fg, Some(Color::Gray));
    }

    #[test]
    fn comments_tab_hides_the_file_path_title() {
        assert!(right_panel_path_title("src/main.rs", true).is_none());
        assert!(right_panel_path_title("src/main.rs", false).is_some());
    }

    #[test]
    fn delete_all_confirmation_shows_confirm_and_cancel_keys() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| draw_delete_all_confirmation(frame, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let contents = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(contents.contains("Confirmation Required"));
        assert!(contents.contains("Are you sure you want to delete all comments?"));

        let popup = centered_popup(buffer.area, 52, 5);
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(bottom_border.contains("Enter Confirm  ·  Esc Cancel"));
    }

    #[test]
    fn repeated_panel_numbers_focus_maximize_and_restore_without_changing_tabs() {
        let mut focus = Focus::Files;
        let mut maximized_panel = None;
        let right_tab = RightTab::Comments;
        let filter_tab = FilterTab::Ignored;

        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Filters);
        assert_eq!(maximized_panel, None);
        assert_eq!(filter_tab, FilterTab::Ignored);
        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, Some(Focus::Filters));
        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, None);

        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Right);
        assert_eq!(right_tab, RightTab::Comments);
        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, Some(Focus::Right));
        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, None);

        activate_numbered_panel(1, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Files);
        assert_eq!(maximized_panel, None);
    }

    #[test]
    fn tab_cycles_only_within_the_focused_panel() {
        let mut right_tab = RightTab::Diff;
        let mut filter_tab = FilterTab::Filters;

        cycle_focused_panel_tab(Focus::Right, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Filters);

        cycle_focused_panel_tab(Focus::Filters, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Ignored);

        cycle_focused_panel_tab(Focus::Files, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Ignored);
    }

    #[test]
    fn filters_take_twenty_percent_of_the_left_column() {
        let panels = left_panel_areas(Rect::new(0, 0, 40, 100));
        assert_eq!(panels[0].height, 80);
        assert_eq!(panels[1].height, 20);
        assert_eq!(panels[1].y, 80);
    }

    #[test]
    fn resize_shortcuts_change_the_focused_panel_width() {
        let mut divider = 34;

        resize_focused_panel(&mut divider, Focus::Files, true, 100);
        assert_eq!(divider, 38);
        resize_focused_panel(&mut divider, Focus::Filters, false, 100);
        assert_eq!(divider, 34);

        resize_focused_panel(&mut divider, Focus::Right, true, 100);
        assert_eq!(divider, 30);
        resize_focused_panel(&mut divider, Focus::Right, false, 100);
        assert_eq!(divider, 34);
    }

    #[test]
    fn resize_shortcuts_preserve_minimum_panel_widths() {
        let mut divider = 24;
        resize_focused_panel(&mut divider, Focus::Files, false, 100);
        assert_eq!(divider, 24);

        divider = 72;
        resize_focused_panel(&mut divider, Focus::Right, false, 100);
        assert_eq!(divider, 72);
    }

    #[test]
    fn diff_scroll_stays_zero_until_content_exceeds_the_viewport() {
        assert_eq!(max_diff_scroll(22, 40), 0);
        assert_eq!(clamped_diff_scroll(12, 22, 40), 0);
        assert_eq!(max_diff_scroll(60, 40), 20);
        assert_eq!(clamped_diff_scroll(30, 60, 40), 20);
    }

    #[test]
    fn horizontal_scroll_moves_both_split_columns_together() {
        let before = "shared-prefix-old-abcdefghijklmnopqrstuvwxyz\n";
        let after = "shared-prefix-new-abcdefghijklmnopqrstuvwxyz\n";
        let mut app = preview_app(before, after);
        app.diff_mode = DiffMode::SideBySide;
        app.diff_rows = line_diff_document(before, after).rows;
        app.maximized_panel = Some(Focus::Right);

        handle_key(&mut app, KeyCode::Right, 50, 8).unwrap();
        assert_eq!(app.diff_horizontal_scroll, 4);
        handle_key(&mut app, KeyCode::Left, 50, 8).unwrap();
        assert_eq!(app.diff_horizontal_scroll, 0);

        app.diff_horizontal_scroll = 6;
        let mut terminal = Terminal::new(TestBackend::new(50, 8)).unwrap();
        terminal
            .draw(|frame| draw_diff(frame, &app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(1, 1)].symbol(), "s");
        assert_eq!(buffer[(26, 1)].symbol(), "s");
        assert!((1..49).any(|x| buffer[(x, 6)].symbol() == "━"));
    }

    #[test]
    fn scrollbar_position_reaches_both_ends_of_overflowing_content() {
        assert_eq!(scrollbar_position(10, 4, 0), 0);
        assert_eq!(scrollbar_position(10, 4, 6), 9);
        assert_eq!(scrollbar_position(4, 4, 0), 0);
    }

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

    #[test]
    fn luminatti_metadata_is_not_shown_as_a_reviewable_change() {
        assert!(is_luminatti_metadata(".luminatti/settings.json"));
        assert!(is_luminatti_metadata(".luminatti/comments.json"));
        assert!(!is_luminatti_metadata("src/.luminatti/settings.json"));
        assert!(!is_luminatti_metadata(".luminatti-example"));
    }

    #[test]
    fn fuzzy_file_search_matches_subsequences_and_keeps_default_order() {
        let files = vec![
            FileItem {
                path: "src/main.rs".into(),
                status: " M".into(),
            },
            FileItem {
                path: "src/file_search.rs".into(),
                status: " M".into(),
            },
            FileItem {
                path: "README.md".into(),
                status: " M".into(),
            },
        ];

        assert_eq!(fuzzy_file_indices(&files, ""), vec![0, 1, 2]);
        assert_eq!(fuzzy_file_indices(&files, "fs"), vec![1]);
        assert!(fuzzy_file_indices(&files, "missing").is_empty());
    }

    #[test]
    fn line_diff_aligns_replacements_and_tracks_line_numbers() {
        let rows = test_diff_rows("a\nb\n", "a\nc\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].old_line, Some(2));
        assert_eq!(rows[1].new_line, Some(2));
        assert_eq!(rows[1].old_text, "b");
        assert_eq!(rows[1].new_text, "c");
        assert!(rows[1].old_changed);
        assert!(rows[1].new_changed);
    }

    #[test]
    fn bracket_navigation_visits_each_changed_row_including_adjacent_changes() {
        let rows = test_diff_rows(
            "same\nold one\nold two\nbetween\nold three\nend\n",
            "same\nnew one\nnew two\nbetween\nnew three\nend\n",
        );

        assert_eq!(changed_row_indices(&rows), vec![1, 2, 4]);
        assert_eq!(adjacent_changed_row(&rows, 1, true), Some(2));
        assert_eq!(adjacent_changed_row(&rows, 2, true), Some(4));
        assert_eq!(adjacent_changed_row(&rows, 4, true), None);
        assert_eq!(adjacent_changed_row(&rows, 4, false), Some(2));
        assert_eq!(adjacent_changed_row(&rows, 2, false), Some(1));
        assert_eq!(adjacent_changed_row(&rows, 1, false), None);
        assert_eq!(adjacent_changed_row(&rows, 3, true), Some(4));
        assert_eq!(adjacent_changed_row(&rows, 3, false), Some(2));
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

    #[test]
    fn agent_comments_accept_hunk_shape() {
        let parsed: AgentCommentFile = serde_json::from_str(
            r#"{"comments":[{"filePath":"a.rs","newLine":9,"summary":"test"}]}"#,
        )
        .unwrap();
        match parsed {
            AgentCommentFile::Store { comments } => assert_eq!(comments[0].new_line, Some(9)),
            _ => panic!("wrong shape"),
        }
    }

    #[test]
    fn selected_local_comment_can_be_removed_without_affecting_others() {
        let comment = |summary: &str| ReviewComment {
            id: new_id(),
            file_path: "src/main.rs".into(),
            old_line: None,
            new_line: Some(1),
            hunk: None,
            summary: summary.into(),
            rationale: None,
            author: None,
            source: "user".into(),
        };
        let mut store = CommentStore {
            version: store_version(),
            comments: vec![comment("first"), comment("second")],
        };

        let removed = remove_local_comment(&mut store, 0).unwrap();
        assert_eq!(removed.summary, "first");
        assert_eq!(store.comments.len(), 1);
        assert_eq!(store.comments[0].summary, "second");
        assert!(remove_local_comment(&mut store, 1).is_none());
    }

    #[test]
    fn same_code_filter_can_hide_or_include_unchanged_rows() {
        let rows = test_diff_rows("same\nold\n", "same\nnew\n");
        assert_eq!(visible_diff_indices(&rows, false), vec![1]);
        assert_eq!(visible_diff_indices(&rows, true), vec![0, 1]);
        assert_eq!(
            rendered_diff_lines(&rows, false, DiffMode::Unified),
            vec![RenderedDiffLine::Row(1), RenderedDiffLine::Row(1)]
        );
    }

    #[test]
    fn changed_tree_hides_collapsed_descendants() {
        let mut root = TreeNode::default();
        root.children
            .entry("src".into())
            .or_default()
            .children
            .entry("main.rs".into())
            .or_default()
            .file_index = Some(0);

        let mut expanded = vec![];
        flatten_tree(&root, "", 0, &BTreeSet::new(), &mut expanded);
        assert_eq!(expanded.len(), 2);
        assert!(expanded[0].expanded);

        let mut collapsed = vec![];
        flatten_tree(
            &root,
            "",
            0,
            &BTreeSet::from(["src".to_owned()]),
            &mut collapsed,
        );
        assert_eq!(collapsed.len(), 1);
        assert!(!collapsed[0].expanded);
    }
}
