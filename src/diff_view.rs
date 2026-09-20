use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

use crate::diff::{DiffRow, DiffSpan, StructuralChange, visible_diff_indices};

const SPLIT_CONTEXT_LINES: usize = 3;
/// Fixed gutter width of a split-view line (`" {:>5} "` and `"    · "`).
pub(crate) const SPLIT_PREFIX_WIDTH: usize = 6;
/// Fixed gutter width of a unified-view line (`"-      "` and `"{marker}{:>5} "`).
pub(crate) const UNIFIED_PREFIX_WIDTH: usize = 7;
/// Width of the `"  ···"` separator shown between distant split hunks.
pub(crate) const SPLIT_SEPARATOR_WIDTH: usize = 5;
pub(crate) const SELECTION_BACKGROUND: Color = Color::Rgb(62, 68, 81);

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

pub(crate) fn pad_selected_line(mut line: Line<'static>, width: u16) -> Line<'static> {
    if line.style.bg == Some(SELECTION_BACKGROUND) {
        let padding = (width as usize).saturating_sub(line.width());
        line.spans.push(Span::raw(" ".repeat(padding)));
    }
    line
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

    visible_diff_indices(rows, show_unchanged)
        .into_iter()
        .flat_map(move |index| {
            let row = &rows[index];
            (0..unified_row_line_count(row)).map(move |occurrence| RenderedDiffLine::Row {
                row: index,
                occurrence,
            })
        })
        .collect()
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
    selected: usize,
    column_limit: usize,
) -> Option<Line<'static>> {
    let renders_deletion = row.old_line.is_some() && (row.old_changed || row.new_line.is_none());
    match occurrence {
        0 if renders_deletion => Some(unified_line(
            index,
            row.old_line,
            source_prefix(&row.old_text, column_limit),
            &row.old_spans,
            '-',
            true,
            selected,
        )),
        occurrence if row.new_line.is_some() && occurrence <= renders_deletion as usize => {
            Some(unified_line(
                index,
                row.new_line,
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
            RenderedDiffLine::Separator => SPLIT_SEPARATOR_WIDTH,
            RenderedDiffLine::Row { row, .. } => match mode {
                DiffMode::SideBySide => split_row_content_width(&rows[*row]),
                DiffMode::Unified => unified_row_content_width(&rows[*row]),
            },
        })
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

pub(crate) fn side_line(
    index: usize,
    line: Option<u32>,
    text: &str,
    spans: &[DiffSpan],
    changed: bool,
    deletion: bool,
    selected: usize,
) -> Line<'static> {
    let prefix = line
        .map(|n| format!("{:>5} ", n))
        .unwrap_or_else(|| "    · ".into());
    let selected = index == selected && line.is_some();
    let prefix_style = if line.is_none() {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else {
        selected_style(changed_style(changed, deletion), selected).add_modifier(Modifier::DIM)
    };
    let mut rendered = vec![Span::styled(prefix, prefix_style)];
    rendered.extend(styled_source_spans(
        text, spans, changed, deletion, selected,
    ));
    Line::from(rendered).style(selected_style(Style::default(), selected))
}

/// All physical lines of a unified row in order; test-side reference for
/// [`unified_occurrence_line`].
#[cfg(test)]
pub(crate) fn unified_lines(index: usize, row: &DiffRow, selected: usize) -> Vec<Line<'static>> {
    (0..unified_row_line_count(row))
        .filter_map(|occurrence| {
            unified_occurrence_line(index, row, occurrence, selected, usize::MAX)
        })
        .collect()
}

fn unified_line(
    index: usize,
    line: Option<u32>,
    text: &str,
    spans: &[DiffSpan],
    marker: char,
    deletion: bool,
    selected: usize,
) -> Line<'static> {
    let selected = index == selected;
    let changed = marker != ' ';
    let prefix_style = selected_style(changed_style(changed, deletion), selected);
    let mut rendered = vec![Span::styled(
        if deletion {
            "-      ".to_owned()
        } else {
            format!("{}{:>5} ", marker, line.unwrap_or(0))
        },
        if deletion {
            selected_style(Style::default().fg(Color::Rgb(165, 112, 116)), selected)
        } else {
            prefix_style
        }
        .add_modifier(Modifier::DIM),
    )];
    let source_spans = styled_source_spans(text, spans, changed, deletion, selected);
    let mut source = Vec::with_capacity(source_spans.len());
    let mut deletion_content_started = false;
    for span in source_spans {
        let mut style = span.style.remove_modifier(Modifier::UNDERLINED);
        if deletion {
            let color = if style.fg == Some(Color::LightRed) {
                Color::Rgb(165, 112, 116)
            } else {
                Color::Rgb(140, 143, 150)
            };
            style = style.fg(color).remove_modifier(Modifier::BOLD);
        }

        let content = span.content.into_owned();
        if !deletion {
            source.push(Span::styled(content, style));
            continue;
        }
        if deletion_content_started {
            source.push(Span::styled(
                content,
                style.add_modifier(Modifier::CROSSED_OUT),
            ));
            continue;
        }
        let Some(content_start) = content
            .char_indices()
            .find_map(|(index, character)| (!character.is_whitespace()).then_some(index))
        else {
            source.push(Span::styled(content, style));
            continue;
        };

        deletion_content_started = true;
        if content_start > 0 {
            source.push(Span::styled(content[..content_start].to_owned(), style));
        }
        source.push(Span::styled(
            content[content_start..].to_owned(),
            style.add_modifier(Modifier::CROSSED_OUT),
        ));
    }
    rendered.extend(source);
    Line::from(rendered).style(selected_style(Style::default(), selected))
}

fn styled_source_spans(
    text: &str,
    spans: &[DiffSpan],
    line_changed: bool,
    deletion: bool,
    selected: bool,
) -> Vec<Span<'static>> {
    let fallback = if line_changed && !spans.iter().any(|span| span.change.is_changed()) {
        changed_style(true, deletion)
    } else {
        Style::default().fg(Color::Gray)
    };
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

fn changed_style(changed: bool, deletion: bool) -> Style {
    if changed {
        Style::default().fg(if deletion {
            Color::LightRed
        } else {
            Color::LightGreen
        })
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn diff_span_style(span: &DiffSpan, deletion: bool) -> Style {
    let changed = span.change.is_changed();
    let mut style = if changed {
        changed_style(true, deletion)
    } else {
        Style::default().fg(Color::Gray)
    };

    if matches!(span.change, StructuralChange::NovelWord) {
        style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    style
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

    fn measured_content_width(rows: &[DiffRow], mode: DiffMode, selected: usize) -> usize {
        let lines = rendered_diff_lines(rows, true, mode);
        lines
            .iter()
            .map(|line| match line {
                RenderedDiffLine::Separator => split_separator_line().width(),
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
            for selected in [usize::MAX, 0, rows.len() - 1] {
                let lines = rendered_diff_lines(&rows, true, mode);
                assert_eq!(
                    rendered_content_width(&rows, &lines, mode),
                    measured_content_width(&rows, mode, selected),
                    "{mode:?} / selected {selected}"
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
                unified_lines(index, row, usize::MAX).len()
            );
        }
    }

    #[test]
    fn missing_split_side_is_not_selected() {
        let missing = side_line(0, None, "", &[], false, true, 0);
        assert_eq!(missing.spans[0].content, "    · ");
        assert_eq!(missing.spans[0].style.fg, Some(Color::DarkGray));
        assert!(missing.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(
            missing
                .spans
                .iter()
                .all(|span| span.style.bg != Some(SELECTION_BACKGROUND))
        );

        let present = side_line(0, Some(1), "added", &[], true, false, 0);
        assert!(
            present
                .spans
                .iter()
                .all(|span| span.style.bg == Some(SELECTION_BACKGROUND))
        );
    }

    #[test]
    fn only_changed_tokens_receive_diff_coloring() {
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

        let line = side_line(0, Some(1), "const new", &spans, true, false, usize::MAX);
        assert_eq!(line.spans[1].style.fg, Some(Color::Gray));
        assert!(!line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans.last().unwrap().style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn unified_view_does_not_underline_changed_words() {
        let spans = vec![DiffSpan {
            start: 0,
            end: 7,
            change: StructuralChange::NovelWord,
            highlight: StructuralHighlight::Normal,
        }];

        for (marker, deletion) in [('-', true), ('+', false)] {
            let line = unified_line(0, Some(1), "changed", &spans, marker, deletion, usize::MAX);
            let changed_word = &line.spans[1];

            assert!(
                !changed_word
                    .style
                    .add_modifier
                    .contains(Modifier::UNDERLINED)
            );
        }
    }

    #[test]
    fn unified_deletion_strikethrough_starts_after_indentation() {
        let text = "    removed";
        let spans = vec![DiffSpan {
            start: 0,
            end: text.len(),
            change: StructuralChange::NovelWord,
            highlight: StructuralHighlight::Normal,
        }];

        let line = unified_line(0, Some(1), text, &spans, '-', true, usize::MAX);

        assert_eq!(line.spans[1].content, "    ");
        assert!(
            !line.spans[1]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
        assert_eq!(line.spans[2].content, "removed");
        assert!(
            line.spans[2]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
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
}
