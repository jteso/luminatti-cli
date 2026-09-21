use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

use crate::diff::{DiffRow, DiffSpan, StructuralChange, visible_diff_indices};

const SPLIT_CONTEXT_LINES: usize = 3;
/// Fixed gutter width of a split-view line (`"-{line:>5} "`).
pub(crate) const SPLIT_PREFIX_WIDTH: usize = 7;
/// Fixed gutter width of a unified-view line (`"-{old:>5} {new:>5} "`).
pub(crate) const UNIFIED_PREFIX_WIDTH: usize = 13;
/// Width of the `"  ···"` separator shown between distant split hunks.
pub(crate) const SPLIT_SEPARATOR_WIDTH: usize = 5;
const UNIFIED_SEPARATOR_WIDTH: usize = UNIFIED_PREFIX_WIDTH + 3;
pub(crate) const SELECTION_BACKGROUND: Color = Color::Rgb(48, 53, 64);
const ADDITION_BACKGROUND: Color = Color::Rgb(30, 53, 42);
const DELETION_BACKGROUND: Color = Color::Rgb(59, 37, 40);
const ADDITION_TOKEN_BACKGROUND: Color = Color::Rgb(63, 111, 70);
const DELETION_TOKEN_BACKGROUND: Color = Color::Rgb(122, 52, 52);
const MISSING_DOT_COLOR: Color = Color::Rgb(76, 82, 94);

/// Stop at the visible right edge before allocating styled text or measuring it.
/// Ratatui's grapheme iterator preserves combining marks and emoji sequences.
pub(crate) fn source_prefix(text: &str, columns: usize) -> &str {
    if columns == usize::MAX {
        return text;
    }
    let span = Span::raw(text);
    let mut width = 0;
    let mut end = 0;
    for grapheme in span.styled_graphemes(Style::default()) {
        if width >= columns {
            break;
        }
        width += unicode_width::UnicodeWidthStr::width(grapheme.symbol);
        end += grapheme.symbol.len();
    }
    &text[..end]
}

pub(crate) fn pad_line_background(mut line: Line<'static>, width: u16) -> Line<'static> {
    if line.style.bg.is_some() {
        let padding = (width as usize).saturating_sub(line.width());
        line.spans.push(Span::raw(" ".repeat(padding)));
    }
    line
}

pub(crate) fn missing_line(width: u16) -> Line<'static> {
    Line::from(Span::styled(
        "·".repeat(width as usize),
        Style::default()
            .fg(MISSING_DOT_COLOR)
            .add_modifier(Modifier::DIM),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiffMode {
    SideBySide,
    Unified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderedDiffLine {
    /// One physical line of `row`. Unified rows can span a deletion and an
    /// insertion line; `occurrence` selects which one (0-based, top down).
    Row {
        row: usize,
        occurrence: usize,
    },
    Separator,
}

impl RenderedDiffLine {
    pub(crate) fn row_index(self) -> Option<usize> {
        match self {
            Self::Row { row, .. } => Some(row),
            Self::Separator => None,
        }
    }
}

pub(crate) fn rendered_diff_lines(
    rows: &[DiffRow],
    show_unchanged: bool,
    mode: DiffMode,
) -> Vec<RenderedDiffLine> {
    if mode == DiffMode::SideBySide {
        return split_hunk_lines(rows, show_unchanged);
    }

    let mut rendered = vec![];
    let mut previous = None;
    for index in visible_diff_indices(rows, show_unchanged) {
        if previous.is_some_and(|previous| index > previous + 1) {
            rendered.push(RenderedDiffLine::Separator);
        }
        rendered.extend((0..unified_row_line_count(&rows[index])).map(|occurrence| {
            RenderedDiffLine::Row {
                row: index,
                occurrence,
            }
        }));
        previous = Some(index);
    }
    rendered
}

/// Number of physical lines a unified view spends on `row`.
pub(crate) fn unified_row_line_count(row: &DiffRow) -> usize {
    let deletions =
        (row.old_line.is_some() && (row.old_changed || row.new_line.is_none())) as usize;
    deletions + row.new_line.is_some() as usize
}

/// Renders exactly the physical line at `occurrence` of a unified row, so a
/// viewport slice starting mid-row still paints the right half of it.
pub(crate) fn unified_occurrence_line(
    index: usize,
    row: &DiffRow,
    occurrence: usize,
    selected: Option<usize>,
    column_limit: usize,
) -> Option<Line<'static>> {
    let renders_deletion = row.old_line.is_some() && (row.old_changed || row.new_line.is_none());
    match occurrence {
        0 if renders_deletion => Some(unified_line(
            index,
            [row.old_line, None],
            source_prefix(&row.old_text, column_limit),
            &row.old_spans,
            '-',
            true,
            selected,
        )),
        occurrence if row.new_line.is_some() && occurrence <= renders_deletion as usize => {
            Some(unified_line(
                index,
                [
                    if row.new_changed { None } else { row.old_line },
                    row.new_line,
                ],
                source_prefix(&row.new_text, column_limit),
                &row.new_spans,
                if row.new_changed { '+' } else { ' ' },
                false,
                selected,
            ))
        }
        _ => None,
    }
}

/// Widest terminal column any rendered line can reach, without materializing
/// styled spans. Row widths are precomputed on the diff itself.
pub(crate) fn rendered_content_width(
    rows: &[DiffRow],
    lines: &[RenderedDiffLine],
    mode: DiffMode,
) -> usize {
    lines
        .iter()
        .map(|line| match line {
            RenderedDiffLine::Separator => match mode {
                DiffMode::SideBySide => SPLIT_SEPARATOR_WIDTH,
                DiffMode::Unified => UNIFIED_SEPARATOR_WIDTH,
            },
            RenderedDiffLine::Row { row, .. } => match mode {
                DiffMode::SideBySide => split_row_content_width(&rows[*row]),
                DiffMode::Unified => unified_row_content_width(&rows[*row]),
            },
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn rendered_source_content_width(rows: &[DiffRow], lines: &[RenderedDiffLine]) -> usize {
    lines
        .iter()
        .filter_map(|line| line.row_index())
        .map(|row| SPLIT_PREFIX_WIDTH + rows[row].new_width as usize)
        .max()
        .unwrap_or(0)
}

fn split_row_content_width(row: &DiffRow) -> usize {
    if row.old_line.is_none() && row.new_line.is_none() {
        return 0;
    }
    SPLIT_PREFIX_WIDTH + row.old_width.max(row.new_width) as usize
}

fn unified_row_content_width(row: &DiffRow) -> usize {
    let deletions =
        (row.old_line.is_some() && (row.old_changed || row.new_line.is_none())) as usize;
    let insertions = row.new_line.is_some() as usize;
    deletions.max(insertions) * (UNIFIED_PREFIX_WIDTH + row.old_width.max(row.new_width) as usize)
}

fn split_hunk_lines(rows: &[DiffRow], show_unchanged: bool) -> Vec<RenderedDiffLine> {
    if show_unchanged {
        return (0..rows.len())
            .map(|row| RenderedDiffLine::Row { row, occurrence: 0 })
            .collect();
    }

    let mut ranges: Vec<(usize, usize)> = vec![];
    for changed in visible_diff_indices(rows, false) {
        let start = changed.saturating_sub(SPLIT_CONTEXT_LINES);
        let end = changed
            .saturating_add(SPLIT_CONTEXT_LINES + 1)
            .min(rows.len());
        if let Some((_, previous_end)) = ranges.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            ranges.push((start, end));
        }
    }

    let mut rendered = vec![];
    for (range_index, (start, end)) in ranges.into_iter().enumerate() {
        if range_index > 0 {
            rendered.push(RenderedDiffLine::Separator);
        }
        rendered.extend((start..end).map(|row| RenderedDiffLine::Row { row, occurrence: 0 }));
    }
    rendered
}

pub(crate) fn split_separator_line() -> Line<'static> {
    Line::from(Span::styled(
        "  ···",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    ))
}

pub(crate) fn unified_separator_line() -> Line<'static> {
    Line::from(Span::styled(
        format!("{}···", " ".repeat(UNIFIED_PREFIX_WIDTH)),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    ))
}

pub(crate) fn side_line(
    index: usize,
    line: Option<u32>,
    text: &str,
    spans: &[DiffSpan],
    changed: bool,
    deletion: bool,
    selected: Option<usize>,
) -> Line<'static> {
    let missing = line.is_none();
    let number = line
        .map(|n| format!("{:>5} ", n))
        .unwrap_or_else(|| "      ".into());
    let selected = Some(index) == selected && line.is_some();
    let changed = changed && line.is_some();
    let number_style = if missing {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else {
        selected_style(gutter_number_style(changed, deletion), selected)
    };
    let mut rendered = vec![
        Span::styled(
            diff_marker(changed, deletion).to_string(),
            selected_style(gutter_marker_style(changed, deletion), selected),
        ),
        Span::styled(number, number_style),
    ];
    rendered.extend(styled_source_spans(text, spans, deletion, selected));
    Line::from(rendered).style(row_style(changed, deletion, selected))
}

/// All physical lines of a unified row in order; test-side reference for
/// [`unified_occurrence_line`].
#[cfg(test)]
pub(crate) fn unified_lines(
    index: usize,
    row: &DiffRow,
    selected: Option<usize>,
) -> Vec<Line<'static>> {
    (0..unified_row_line_count(row))
        .filter_map(|occurrence| {
            unified_occurrence_line(index, row, occurrence, selected, usize::MAX)
        })
        .collect()
}

fn unified_line(
    index: usize,
    line_numbers: [Option<u32>; 2],
    text: &str,
    spans: &[DiffSpan],
    marker: char,
    deletion: bool,
    selected: Option<usize>,
) -> Line<'static> {
    let selected = Some(index) == selected;
    let changed = marker != ' ';
    let mut rendered = vec![
        Span::styled(
            marker.to_string(),
            selected_style(gutter_marker_style(changed, deletion), selected),
        ),
        Span::styled(
            gutter_numbers(line_numbers),
            selected_style(gutter_number_style(changed, deletion), selected),
        ),
    ];
    rendered.extend(styled_source_spans(text, spans, deletion, selected));
    Line::from(rendered).style(row_style(changed, deletion, selected))
}

fn styled_source_spans(
    text: &str,
    spans: &[DiffSpan],
    deletion: bool,
    selected: bool,
) -> Vec<Span<'static>> {
    let fallback = Style::default().fg(Color::Gray);
    let mut rendered = vec![];
    let mut cursor = 0;

    for span in spans {
        if span.start >= text.len() {
            break;
        }
        let start = span.start.max(cursor).min(text.len());
        let end = span.end.min(text.len());
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        if cursor < start {
            rendered.push(Span::styled(
                text[cursor..start].to_owned(),
                selected_style(fallback, selected),
            ));
        }
        rendered.push(Span::styled(
            text[start..end].to_owned(),
            selected_style(diff_span_style(span, deletion), selected),
        ));
        cursor = end;
    }

    if cursor < text.len() {
        rendered.push(Span::styled(
            text[cursor..].to_owned(),
            selected_style(fallback, selected),
        ));
    } else if rendered.is_empty() {
        rendered.push(Span::styled(
            String::new(),
            selected_style(fallback, selected),
        ));
    }
    rendered
}

fn diff_marker(changed: bool, deletion: bool) -> char {
    match (changed, deletion) {
        (true, true) => '-',
        (true, false) => '+',
        (false, _) => ' ',
    }
}

fn diff_span_style(span: &DiffSpan, deletion: bool) -> Style {
    let mut style = Style::default().fg(Color::Gray);

    if span.change.is_changed() {
        style = style.bg(if deletion {
            DELETION_TOKEN_BACKGROUND
        } else {
            ADDITION_TOKEN_BACKGROUND
        });
    }
    if matches!(span.change, StructuralChange::NovelWord) {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn gutter_numbers([old_line, new_line]: [Option<u32>; 2]) -> String {
    let old = old_line.map_or_else(String::new, |line| line.to_string());
    let new = new_line.map_or_else(String::new, |line| line.to_string());
    format!("{old:>5} {new:>5} ")
}

fn gutter_marker_style(changed: bool, deletion: bool) -> Style {
    if !changed {
        return Style::default().fg(Color::DarkGray);
    }
    Style::default()
        .fg(if deletion {
            Color::LightRed
        } else {
            Color::LightGreen
        })
        .add_modifier(Modifier::BOLD)
}

fn gutter_number_style(changed: bool, deletion: bool) -> Style {
    let color = if changed {
        if deletion {
            Color::LightRed
        } else {
            Color::LightGreen
        }
    } else {
        Color::DarkGray
    };
    Style::default().fg(color).add_modifier(Modifier::DIM)
}

fn row_style(changed: bool, deletion: bool, selected: bool) -> Style {
    let style = if changed {
        Style::default().bg(if deletion {
            DELETION_BACKGROUND
        } else {
            ADDITION_BACKGROUND
        })
    } else {
        Style::default()
    };
    selected_style(style, selected)
}

fn selected_style(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(SELECTION_BACKGROUND)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{StructuralHighlight, line_diff_document};

    #[test]
    fn source_clipping_preserves_graphemes_at_the_viewport_edge() {
        assert_eq!(source_prefix("a界z", 2), "a界");
        assert_eq!(source_prefix("e\u{301}x", 1), "e\u{301}");
        assert_eq!(source_prefix("👩‍💻x", 2), "👩‍💻");
        assert_eq!(source_prefix("text", 0), "");
    }

    fn measured_content_width(rows: &[DiffRow], mode: DiffMode, selected: Option<usize>) -> usize {
        let lines = rendered_diff_lines(rows, true, mode);
        lines
            .iter()
            .map(|line| match line {
                RenderedDiffLine::Separator => match mode {
                    DiffMode::SideBySide => split_separator_line().width(),
                    DiffMode::Unified => unified_separator_line().width(),
                },
                RenderedDiffLine::Row { row, .. } => {
                    let index = *row;
                    let row = &rows[index];
                    match mode {
                        DiffMode::SideBySide => [
                            side_line(
                                index,
                                row.old_line,
                                &row.old_text,
                                &row.old_spans,
                                row.old_changed,
                                true,
                                selected,
                            )
                            .width(),
                            side_line(
                                index,
                                row.new_line,
                                &row.new_text,
                                &row.new_spans,
                                row.new_changed,
                                false,
                                selected,
                            )
                            .width(),
                        ]
                        .into_iter()
                        .max()
                        .unwrap(),
                        DiffMode::Unified => unified_lines(index, row, selected)
                            .iter()
                            .map(Line::width)
                            .max()
                            .unwrap(),
                    }
                }
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn content_width_matches_the_widths_of_materialized_lines() {
        let before = "short\nexport function mapEmployee(employee: PreparedEmployee): PayrollEmployee {\n  values\n}\nwideré — πsum\n";
        let after = "short\nexport function mapEmployee(\n  employee: PreparedEmployee,\n): PayrollEmployee {\n  values\n}\nwide line moved over here\n";
        let rows = line_diff_document(before, after).rows;

        for mode in [DiffMode::Unified, DiffMode::SideBySide] {
            for selected in [None, Some(0), Some(rows.len() - 1)] {
                let lines = rendered_diff_lines(&rows, true, mode);
                assert_eq!(
                    rendered_content_width(&rows, &lines, mode),
                    measured_content_width(&rows, mode, selected),
                    "{mode:?} / selected {selected:?}"
                );
            }
        }
    }

    #[test]
    fn unified_line_count_matches_materialized_lines() {
        let rows = line_diff_document("a\nb\nc\n", "a\nb2\nc\n").rows;
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(
                unified_row_line_count(row),
                unified_lines(index, row, None).len()
            );
        }
    }

    #[test]
    fn missing_split_side_is_not_selected() {
        let missing = side_line(0, None, "", &[], false, true, Some(0));
        assert_eq!(missing.spans[0].content, " ");
        assert_eq!(missing.spans[1].content, "      ");
        assert_eq!(missing.spans[1].style.fg, Some(Color::DarkGray));
        assert!(missing.spans[1].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(missing.style.bg, None);
        assert!(!missing.to_string().contains('·'));
        assert!(
            missing
                .spans
                .iter()
                .all(|span| span.style.bg != Some(SELECTION_BACKGROUND))
        );

        let present = side_line(0, Some(1), "added", &[], true, false, Some(0));
        assert!(
            present
                .spans
                .iter()
                .all(|span| span.style.bg == Some(SELECTION_BACKGROUND))
        );
    }

    #[test]
    fn missing_line_uses_a_dense_visible_pattern() {
        let line = missing_line(12);

        assert_eq!(line.to_string(), "············");
        assert_eq!(line.spans[0].style.fg, Some(Color::Rgb(76, 82, 94)));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn row_and_token_styles_express_different_diff_detail() {
        let spans = vec![
            DiffSpan {
                start: 0,
                end: 5,
                change: StructuralChange::Unchanged,
                highlight: StructuralHighlight::Keyword,
            },
            DiffSpan {
                start: 6,
                end: 9,
                change: StructuralChange::Novel,
                highlight: StructuralHighlight::Normal,
            },
        ];

        let line = side_line(0, Some(1), "const new", &spans, true, false, None);
        assert_eq!(line.style.bg, Some(ADDITION_BACKGROUND));
        assert_eq!(line.spans[0].style.fg, Some(Color::LightGreen));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[2].style.fg, Some(Color::Gray));
        assert_eq!(line.spans[2].style.bg, None);
        assert_eq!(
            line.spans.last().unwrap().style.bg,
            Some(ADDITION_TOKEN_BACKGROUND)
        );
    }

    #[test]
    fn unified_view_uses_token_background_without_text_decoration() {
        let spans = vec![DiffSpan {
            start: 0,
            end: 7,
            change: StructuralChange::NovelWord,
            highlight: StructuralHighlight::Normal,
        }];

        for (marker, deletion) in [('-', true), ('+', false)] {
            let (old_line, new_line) = if deletion {
                (Some(1), None)
            } else {
                (None, Some(1))
            };
            let line = unified_line(
                0,
                [old_line, new_line],
                "changed",
                &spans,
                marker,
                deletion,
                None,
            );
            let changed_word = &line.spans[2];

            assert!(
                !changed_word
                    .style
                    .add_modifier
                    .contains(Modifier::UNDERLINED)
            );
            assert!(
                !changed_word
                    .style
                    .add_modifier
                    .contains(Modifier::CROSSED_OUT)
            );
            assert!(changed_word.style.bg.is_some());
        }
    }

    #[test]
    fn changed_row_background_pads_to_the_viewport_width() {
        let text = "    removed";
        let spans = vec![DiffSpan {
            start: 0,
            end: text.len(),
            change: StructuralChange::NovelWord,
            highlight: StructuralHighlight::Normal,
        }];

        let line = unified_line(0, [Some(1), None], text, &spans, '-', true, None);
        let padded = pad_line_background(line, 40);

        assert_eq!(padded.style.bg, Some(DELETION_BACKGROUND));
        assert_eq!(padded.width(), 40);
        assert_eq!(padded.spans[2].content, text);
        assert!(
            padded
                .spans
                .iter()
                .all(|span| !span.style.add_modifier.contains(Modifier::CROSSED_OUT))
        );
    }

    #[test]
    fn split_view_groups_distant_changes_into_contextual_hunks() {
        let before = (1..=24)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let after = before
            .replace("line 5", "changed 5")
            .replace("line 20", "changed 20");
        let rows = line_diff_document(&before, &after).rows;

        let rendered = rendered_diff_lines(&rows, false, DiffMode::SideBySide);
        assert_eq!(
            rendered,
            [
                RenderedDiffLine::Row {
                    row: 1,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 2,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 3,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 4,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 5,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 6,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 7,
                    occurrence: 0
                },
                RenderedDiffLine::Separator,
                RenderedDiffLine::Row {
                    row: 16,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 17,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 18,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 19,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 20,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 21,
                    occurrence: 0
                },
                RenderedDiffLine::Row {
                    row: 22,
                    occurrence: 0
                },
            ]
        );
    }

    #[test]
    fn unified_view_separates_distant_change_hunks() {
        let rows = line_diff_document(
            "same\nold one\nbetween\nold two\nend\n",
            "same\nnew one\nbetween\nnew two\nend\n",
        )
        .rows;

        let rendered = rendered_diff_lines(&rows, false, DiffMode::Unified);
        assert_eq!(
            rendered
                .iter()
                .filter(|line| matches!(line, RenderedDiffLine::Separator))
                .count(),
            1
        );
        assert_eq!(unified_separator_line().width(), UNIFIED_SEPARATOR_WIDTH);
    }
}
