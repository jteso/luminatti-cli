//! Background diff loading and cached render geometry.
use super::{
    App,
    navigation::{clamped_diff_scroll, nearest_source_row},
    worker::Worker,
};
use crate::{
    diff::{DiffRow, line_diff_document, source_document, structural_diff_document},
    diff_view::{
        DiffMode, RenderedDiffLine, rendered_content_width, rendered_diff_lines,
        rendered_source_content_width,
    },
    git::{git_show_head_file, head_commit},
};
use anyhow::Result;
use std::{
    cell::Ref,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_FILES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiffRequest {
    path: String,
    mode: DiffMode,
}

pub(super) struct PendingDiff {
    request: DiffRequest,
    generation: u64,
}

pub(super) type DiffWorker = Worker<DiffRequest, Result<Arc<LoadedDiff>>>;

#[derive(Debug)]
pub(super) struct LoadedDiff {
    rows: Arc<Vec<DiffRow>>,
    final_rows: Arc<Vec<DiffRow>>,
    language: String,
    has_syntactic_changes: bool,
    signature: u64,
    geometry: [Arc<DiffRenderState>; 3],
    bytes: usize,
}

struct CacheEntry {
    request: DiffRequest,
    head: String,
    stamp: Option<(SystemTime, u64)>,
    document: Arc<LoadedDiff>,
}

fn file_stamp(path: &Path) -> std::io::Result<Option<(SystemTime, u64)>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some((metadata.modified()?, metadata.len()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn diff_worker(repo: PathBuf) -> std::io::Result<DiffWorker> {
    let mut cache: Vec<CacheEntry> = Vec::new();
    let mut revision = 0;
    Worker::spawn("diff-loader", move |request: DiffRequest| {
        let head = head_commit(&repo);
        let stamp = file_stamp(&repo.join(&request.path))?;
        if let Some(index) = cache.iter().position(|entry| {
            entry.request == request && entry.head == head && entry.stamp == stamp
        }) {
            let entry = cache.remove(index);
            let result = Arc::clone(&entry.document);
            cache.push(entry);
            return Ok(result);
        }
        let before = git_show_head_file(&repo, &request.path)?;
        let after = match fs::read_to_string(repo.join(&request.path)) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let document = match request.mode {
            DiffMode::SideBySide => line_diff_document(&before, &after),
            DiffMode::Unified => {
                structural_diff_document(Path::new(&request.path), &before, &after)
            }
        };
        let final_rows = source_document(&after);
        revision += 1;
        let geometry =
            [(false, false), (false, true), (true, true)].map(|(final_view, show_unchanged)| {
                Arc::new(DiffRenderState::new(
                    DiffRenderKey {
                        signature: Some(revision),
                        mode: request.mode,
                        final_view,
                        show_unchanged,
                    },
                    if final_view {
                        &final_rows
                    } else {
                        &document.rows
                    },
                ))
            });
        let bytes = document
            .rows
            .iter()
            .chain(final_rows.iter())
            .map(|row| {
                std::mem::size_of::<DiffRow>()
                    + row.old_text.capacity()
                    + row.new_text.capacity()
                    + (row.old_spans.capacity() + row.new_spans.capacity())
                        * std::mem::size_of::<crate::diff::DiffSpan>()
            })
            .sum::<usize>()
            + geometry
                .iter()
                .map(|state| {
                    state.lines.capacity() * std::mem::size_of::<RenderedDiffLine>()
                        + state.indices.capacity() * std::mem::size_of::<usize>()
                })
                .sum::<usize>();
        let loaded = Arc::new(LoadedDiff {
            rows: Arc::new(document.rows),
            final_rows: Arc::new(final_rows),
            language: document.language,
            has_syntactic_changes: document.has_syntactic_changes,
            signature: revision,
            geometry,
            bytes,
        });
        cache.retain(|entry| entry.request != request);
        while !cache.is_empty()
            && (cache.len() >= CACHE_FILES
                || cache
                    .iter()
                    .map(|entry| entry.document.bytes)
                    .sum::<usize>()
                    + bytes
                    > CACHE_BYTES)
        {
            cache.remove(0);
        }
        // Keep the active document even when it alone exceeds the cache budget.
        cache.push(CacheEntry {
            request,
            head,
            stamp,
            document: Arc::clone(&loaded),
        });
        Ok(loaded)
    })
}

#[derive(Debug)]
pub(super) struct DiffRenderState {
    key: DiffRenderKey,
    pub(super) lines: Vec<RenderedDiffLine>,
    pub(super) indices: Vec<usize>,
    pub(super) content_width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DiffRenderKey {
    signature: Option<u64>,
    mode: DiffMode,
    final_view: bool,
    show_unchanged: bool,
}

impl DiffRenderState {
    fn new(key: DiffRenderKey, rows: &[DiffRow]) -> Self {
        let lines = rendered_diff_lines(rows, key.final_view || key.show_unchanged, key.mode);
        let indices = dedupe_row_indices(&lines);
        let content_width = if key.final_view {
            rendered_source_content_width(rows, &lines)
        } else {
            rendered_content_width(rows, &lines, key.mode)
        };
        Self {
            key,
            lines,
            indices,
            content_width,
        }
    }
}

impl App {
    pub(super) fn rebuild_diff(&mut self) -> Result<()> {
        let Some(path) = self.active_path().map(str::to_owned) else {
            self.pending_diff = None;
            self.reset_diff();
            return Ok(());
        };
        let request = DiffRequest {
            path,
            mode: self.diff_mode,
        };
        if self
            .pending_diff
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            return Ok(());
        }
        if self.active_diff_path.as_deref() != Some(&request.path) {
            self.reset_diff();
            self.diff_anchor = None;
        }
        if self.diff_worker.is_none() {
            self.diff_worker = Some(diff_worker(self.repo.clone())?);
        }
        let generation = self
            .diff_worker
            .as_mut()
            .expect("worker initialized")
            .request(request.clone());
        self.pending_diff = Some(PendingDiff {
            request,
            generation,
        });
        Ok(())
    }

    pub(super) fn poll_diff(&mut self) -> bool {
        let Some((generation, result)) = self.diff_worker.as_ref().and_then(Worker::take) else {
            return false;
        };
        if !self
            .pending_diff
            .as_ref()
            .is_some_and(|pending| pending.generation == generation)
        {
            return false;
        }
        let pending = self.pending_diff.take().expect("matching generation");
        if self.active_path() != Some(pending.request.path.as_str())
            || self.diff_mode != pending.request.mode
        {
            return false;
        }
        let loaded = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.message = format!("{}: {error}", pending.request.path);
                return false;
            }
        };
        if self.diff_signature == Some(loaded.signature) {
            return false;
        }
        self.diff_rows = Arc::clone(&loaded.rows);
        self.final_rows = Arc::clone(&loaded.final_rows);
        self.diff_language = loaded.language.clone();
        self.diff_has_syntactic_changes = loaded.has_syntactic_changes;
        self.diff_signature = Some(loaded.signature);
        self.active_diff_path = Some(pending.request.path);
        self.loaded_diff = Some(loaded);
        self.selected_row = self
            .diff_rows
            .iter()
            .position(DiffRow::is_changed)
            .unwrap_or(0);
        if let Some((old_line, new_line)) = self.diff_anchor.take() {
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
        if self.select_last_change {
            self.selected_row = self
                .diff_rows
                .iter()
                .rposition(DiffRow::is_changed)
                .unwrap_or(0);
            self.select_last_change = false;
        }
        if self.final_view {
            self.selected_row = nearest_source_row(
                &self.final_rows,
                self.diff_rows
                    .get(self.selected_row)
                    .and_then(|row| row.new_line),
            );
        }
        self.final_origin = None;
        self.diff_scroll = 0;
        self.diff_horizontal_scroll = 0;
        self.diff_selection_active = true;
        true
    }

    fn reset_diff(&mut self) {
        self.diff_rows = Arc::default();
        self.final_rows = Arc::default();
        self.loaded_diff = None;
        self.render_state_slot.get_mut().take();
        self.final_origin = None;
        self.diff_language.clear();
        self.diff_has_syntactic_changes = false;
        self.selected_row = 0;
        self.diff_selection_active = false;
        self.diff_scroll = 0;
        self.diff_horizontal_scroll = 0;
        self.diff_signature = None;
        self.active_diff_path = None;
        self.select_last_change = false;
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
        self.reset_diff();
        self.rebuild_diff()?;
        self.diff_anchor = anchor;
        Ok(())
    }

    pub(super) fn active_rows(&self) -> &[DiffRow] {
        if self.final_view {
            &self.final_rows
        } else {
            &self.diff_rows
        }
    }

    /// Render geometry for the current view state, rebuilt only when the
    /// underlying diff or a view flag changed. Callers hold the returned
    /// guard only as long as they stay immutable.
    pub(super) fn render_state(&self) -> Ref<'_, DiffRenderState> {
        let key = DiffRenderKey {
            signature: self.diff_signature,
            mode: self.diff_mode,
            final_view: self.final_view,
            show_unchanged: self.final_view || self.show_unchanged,
        };
        if let Ok(state) = self.render_state_slot.try_borrow()
            && state.as_ref().is_some_and(|state| state.key == key)
        {
            return Ref::map(state, |state| {
                state.as_ref().expect("matching key implies state").as_ref()
            });
        }

        let cached = self.loaded_diff.as_ref().map(|loaded| {
            let index = if self.final_view {
                2
            } else {
                usize::from(self.show_unchanged)
            };
            Arc::clone(&loaded.geometry[index])
        });
        *self.render_state_slot.borrow_mut() =
            Some(cached.unwrap_or_else(|| Arc::new(DiffRenderState::new(key, self.active_rows()))));
        Ref::map(
            self.render_state_slot.borrow(),
            |slot: &Option<Arc<DiffRenderState>>| {
                slot.as_ref().expect("state just stored").as_ref()
            },
        )
    }

    #[cfg(test)]
    pub(super) fn displayed_indices(&self) -> Vec<usize> {
        self.render_state().indices.clone()
    }

    #[cfg(test)]
    pub(super) fn rendered_lines(&self) -> Vec<RenderedDiffLine> {
        self.render_state().lines.clone()
    }

    pub(super) fn rendered_line_count(&self) -> usize {
        self.render_state().lines.len()
    }

    pub(super) fn diff_content_width(&self) -> usize {
        self.render_state().content_width
    }

    pub(super) fn toggle_final_view(&mut self, viewport_height: u16) {
        let offset = {
            let state = self.render_state();
            let selected_position = selected_line_position(&state.lines, self.selected_row);
            selected_position
                .saturating_sub(self.diff_scroll)
                .min(viewport_height.saturating_sub(1) as usize)
        };
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
            let target = {
                let state = self.render_state();
                state
                    .indices
                    .iter()
                    .copied()
                    .min_by_key(|&index| {
                        self.diff_rows[index]
                            .new_line
                            .map_or(u32::MAX, |line| line.abs_diff(anchor.unwrap_or(1)))
                    })
                    .unwrap_or(0)
            };
            self.selected_row = target;
        }
        let (position, content_length) = {
            let state = self.render_state();
            let position = selected_line_position(&state.lines, self.selected_row);
            (position, state.lines.len())
        };
        self.diff_scroll = clamped_diff_scroll(
            position.saturating_sub(offset),
            content_length,
            viewport_height,
        );
    }
}

pub(super) fn selected_line_position(lines: &[RenderedDiffLine], selected_row: usize) -> usize {
    lines
        .iter()
        .position(|line| line.row_index() == Some(selected_row))
        .unwrap_or(0)
}

fn dedupe_row_indices(lines: &[RenderedDiffLine]) -> Vec<usize> {
    lines
        .iter()
        .filter_map(|line| line.row_index())
        .fold(Vec::new(), |mut indices, index| {
            if indices.last() != Some(&index) {
                indices.push(index);
            }
            indices
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_from_a_previous_selection_cannot_replace_the_current_request() {
        use std::{sync::mpsc, time::Duration};
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut app = super::super::test_support::preview_app("", "");
        app.files = ["old.txt", "new.txt"]
            .map(|path| crate::git::FileItem {
                path: path.into(),
                status: "??".into(),
            })
            .to_vec();
        app.diff_worker = Some(
            Worker::spawn("controlled-diff", move |request: DiffRequest| {
                started_tx.send(request.path.clone()).unwrap();
                release_rx.recv().unwrap();
                anyhow::bail!("failed to read {}", request.path)
            })
            .unwrap(),
        );
        app.select_file(0).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "old.txt"
        );
        app.select_file(1).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "new.txt"
        );
        app.poll_diff();
        assert!(app.message.is_empty());
        assert!(app.pending_diff.is_some());
        assert!(app.diff_rows.is_empty());
        release_tx.send(()).unwrap();
        super::super::test_support::wait_for_diff(&mut app);
        assert!(app.message.starts_with("new.txt:"));
    }
    #[test]
    fn render_state_rebuilds_only_when_the_view_state_changes() {
        let mut app = super::super::test_support::preview_app("same\nold\n", "same\nnew\n");
        app.diff_signature = Some(42);

        let first = app.render_state();
        assert_eq!(first.indices, vec![1]);
        let key = first.key;
        drop(first);
        assert!(app.render_state_slot.borrow().is_some());

        // Identical view state: the cached derivation is reused as-is.
        assert_eq!(app.render_state().key, key);

        app.show_unchanged = true;
        let expanded = app.render_state();
        assert_eq!(expanded.indices, vec![0, 1]);
        assert!(expanded.key != key);
    }

    #[test]
    fn render_state_matches_a_fresh_derivation_for_both_modes() {
        let mut app =
            super::super::test_support::preview_app("alpha\nbeta\n", "alpha\ngamma\ndelta\n");
        app.show_unchanged = true;
        let rows = app.active_rows().to_vec();
        for mode in [DiffMode::Unified, DiffMode::SideBySide] {
            app.diff_mode = mode;
            let state = app.render_state();
            let expected_lines = rendered_diff_lines(&rows, true, mode);
            assert_eq!(state.lines, expected_lines);
            assert_eq!(
                state.content_width,
                rendered_content_width(&rows, &state.lines, mode)
            );
        }
    }

    #[test]
    fn display_indices_skip_repeated_unified_rows() {
        let app = super::super::test_support::preview_app("same\nold\n", "same\nnew\n");
        let state = app.render_state();
        assert_eq!(
            state.lines,
            [
                RenderedDiffLine::Row {
                    row: 1,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 1,
                    occurrence: 1
                },
            ]
        );
        assert_eq!(state.indices, vec![1]);
    }
}
