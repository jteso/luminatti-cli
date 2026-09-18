# Luminatti

Luminatti is a compact, keyboard-first Git worktree diff TUI with mouse support,
resizable panes, stable line-aligned split and unified diffs, local review notes,
and agent-readable comments.

```sh
cargo run --release --bin luminatti -- .
cargo run --release --bin luminatti -- /path/to/repository
```

The target must be inside a Git worktree. Luminatti watches changes by default.
It never changes source files or `.gitignore`.

To install the optimized executable globally for your user:

```sh
./install.sh
```

This installs `luminatti` to `~/.local/bin` by default. Set `BIN_DIR` or
`PREFIX` to choose a different location.

## Keys

`Tab` cycles panel focus; `h`/`l` focus the left files or right diff pane.
`1` focuses Files, `2` Changes/Comments, and `3` Filters; `f` Files, `g`
Filters, `o` Ignored, `d` Changes, `m` Comments, `u` unified, `s` split,
`i` shows or hides common lines, `c` adds a comment, `y` copies the selected
comment, `a` adds a filter, `x` removes one, `r` refreshes, `?` opens the
grouped keybinding help, and `q` quits. Use the arrow keys to move. `/` opens
a fuzzy changed-file picker; type to filter and press `Enter` to open the
selected file. Drag the vertical divider to resize panels.
The Files pane is a changed-only directory tree; `Enter` toggles a directory
and opens the selected file's diff. Filters remain visible in their own panel
at the bottom of the left column. Its Ignored tab lists changed files excluded
by the configured filter globs.

## Metadata and agents

All metadata stays in `.luminatti/`; Luminatti does not manage `.gitignore`.

- `.luminatti/comments.json` contains human comments created in the TUI.
- `.luminatti/agent-comments.json` is watched read-only and accepts a
  Hunk-compatible JSON batch: `{ "comments": [{ "filePath": "src/a.rs",
  "newLine": 12, "summary": "...", "rationale": "...", "author": "agent" }] }`.
- `.luminatti/filters.json` contains project-local glob exclusions.

Agent comments and local notes are anchored to file paths and old/new lines;
they never modify code.
