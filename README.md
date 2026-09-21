# Luminatti

Luminatti is a compact, keyboard-first Git worktree diff TUI with mouse support,
resizable panes, line-aligned split diffs, Difftastic-powered structural unified
diffs, local review notes, and agent-readable comments.

Difftastic and its Tree-sitter parsers are embedded in the binary. No separate
`difft` installation is required. Unified view preserves Difftastic's structural
line alignment and token-level change emphasis while using a neutral source
palette. Split view uses predictable physical-line alignment with word-level
change emphasis, three lines of surrounding context, and collapsed-gap hunk
separators. Difftastic's native line-oriented fallbacks remain available for
unsupported or unparsable inputs in unified view.

File loading, diff calculation, and Git status refresh run in background workers.
Rapid file navigation keeps only the latest pending request; completed diffs are
cached with a 64 MiB budget (plus an oversized active file). Files over 1 MB use
line matching with a shared 100 ms matching budget. If matching reaches that
budget, highlighting becomes coarser while all source lines remain available.
Rendering only styles the visible rows and columns.

```sh
cargo run --release --bin luminatti -- .
cargo run --release --bin luminatti -- /path/to/repository
```

The target must be inside a Git worktree. Luminatti watches changes by default.
It never changes source files or `.gitignore`.

## Install with Homebrew

Once a release is published, install Luminatti on macOS with:

```sh
brew install jteso/tap/luminatti
```

Maintainers can follow the [release guide](docs/releasing.md) to publish a new
version and update the formula automatically.

To install the optimized executable globally for your user:

```sh
./install.sh
```

This installs `luminatti` to `~/.local/bin` by default. Set `BIN_DIR` or
`PREFIX` to choose a different location.

## Keys

`Tab` cycles tabs within the focused panel and never changes panel focus;
`h`/`l` focus the left files or right diff pane, except when unified Changes is
active, where `h` toggles Diff / Final from any focused panel. `1`, `2`, and `3` focus their
numbered panels; pressing the focused panel's number again maximizes it, and a
third press restores the normal layout. Use `<`/`>` to narrow or widen the focused
panel. `[`/`]` move to the previous/next changed row, crossing into the adjacent
changed file at file boundaries; `{`/`}` jump directly to the previous/next
changed file. `f` focuses Files, `u` selects unified view,
and `s` selects split view. In Changes, `Left`/`Right` scroll code horizontally;
split columns share one offset and move together. `Esc` clears the selected diff
line while keeping it as the navigation anchor. In the focused Comments tab,
`d` deletes the selected local comment and `D` deletes all local comments;
the latter asks for `Enter` confirmation and accepts `Esc` to cancel.
Agent-provided comments remain read-only.
`i` shows or hides common lines, `c` adds a comment, `y` copies the selected
comment, `a` adds a filter, `x` removes one, `r` refreshes, `?` opens the
grouped keybinding help, and `q` quits. Use the arrow keys to move. `/` opens
a fuzzy changed-file picker; type to filter and press `Enter` to open the
selected file. Drag the vertical divider to resize panels.
In unified view, removed lines have an empty number gutter, muted text, and
strikethrough. Final shows the complete current file with neutral text and normal
line numbers. Toggling preserves your place and your common-line preference;
`i` only applies to Diff. Use `f` or `1` to focus Files while in unified view.
Change navigation (`[` / `]`) returns to Diff so removed lines remain reachable.
The Files pane is a changed-only directory tree; `Enter` toggles a directory
and opens the selected file's diff. Filters remain visible in their own panel
at the bottom of the left column. Its Ignored tab lists changed files excluded
by the configured filter globs.

## Metadata and agents

All metadata stays in `.luminatti/`; Luminatti does not manage `.gitignore` and
does not show its own metadata in the review tree.

- `.luminatti/comments.json` contains human comments created in the TUI.
- `.luminatti/agent-comments.json` is watched read-only and accepts a
  Hunk-compatible JSON batch: `{ "comments": [{ "filePath": "src/a.rs",
  "newLine": 12, "summary": "...", "rationale": "...", "author": "agent" }] }`.
- `.luminatti/filters.json` contains project-local glob exclusions.
- `.luminatti/settings.json` remembers project-local viewing preferences such
  as unified/split mode, common-line visibility, and divider width.

Agent comments and local notes are anchored to file paths and old/new lines;
they never modify code.

## Development

The binary entry point in `src/main.rs` only parses arguments, locates the Git
worktree, and starts the application. Code is organized by responsibility:

| Location | Responsibility |
| --- | --- |
| `src/app/mod.rs` | Application state, initialization, and refresh orchestration |
| `src/app/terminal.rs` | Terminal session and event loop |
| `src/app/keyboard.rs`, `mouse.rs` | Input dispatch and mouse hit testing |
| `src/app/navigation.rs`, `layout.rs` | Selection, scrolling, and shared pane geometry |
| `src/app/diff.rs`, `files.rs`, `review.rs` | Application actions for diffs, file selection, and review metadata |
| `src/app/ui/` | Read-only rendering for panels, diffs, footer, dialogs, and shared widgets |
| `src/git.rs` | Git subprocesses, worktree discovery, and branch status |
| `src/comments.rs`, `filters.rs`, `settings.rs`, `storage.rs` | Metadata schemas and JSON persistence |
| `src/file_tree.rs`, `search.rs` | File-tree construction and fuzzy matching |
| `src/diff.rs`, `diff_view.rs` | Diff generation, row alignment, and styled diff lines |

Application internals stay private to `app`; its only entry point is `app::run`.
Rendering takes an immutable application reference. Git, persistence, and file
algorithms do not depend on application state. Keep new behavior in the module
that owns it, and put tests beside that code. Cross-module UI tests use Ratatui's
in-memory backend and the fixtures in `src/app/test_support.rs`.

Run the application checks with:

```sh
./tests/vendored_parser_sources.sh
cargo fmt --check
cargo test --bin luminatti --locked
cargo clippy --bin luminatti --all-targets --no-deps --locked -- -D warnings
```

`vendor/difftastic` is the embedded upstream diff engine and is maintained
separately from the application's modules.
