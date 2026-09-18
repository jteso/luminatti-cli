use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

use crate::diff::{DiffRow, DiffSpan, StructuralChange, visible_diff_indices};

const SPLIT_CONTEXT_LINES: usize = 3;
pub(crate) const SELECTION_BACKGROUND: Color = Color::Rgb(62, 68, 81);

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
    Row(usize),
    Separator,
}

impl RenderedDiffLine {
    pub(crate) fn row_index(self) -> Option<usize> {
        match self {
            Self::Row(index) => Some(index),
            Self::Separator => None,
        }
    }
}

pub(crate) fn displayed_diff_indices(
    rows: &[DiffRow],
    show_unchanged: bool,
    mode: DiffMode,
) -> Vec<usize> {
    rendered_diff_lines(rows, show_unchanged, mode)
        .into_iter()
        .filter_map(RenderedDiffLine::row_index)
        .fold(Vec::new(), |mut indices, index| {
            if indices.last() != Some(&index) {
                indices.push(index);
            }
            indices
        })
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
        .flat_map(|index| {
            std::iter::repeat_n(
                RenderedDiffLine::Row(index),
                unified_lines(index, &rows[index], usize::MAX).len(),
            )
        })
        .collect()
}

fn split_hunk_lines(rows: &[DiffRow], show_unchanged: bool) -> Vec<RenderedDiffLine> {
    if show_unchanged {
        return (0..rows.len()).map(RenderedDiffLine::Row).collect();
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
        rendered.extend((start..end).map(RenderedDiffLine::Row));
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

pub(crate) fn unified_lines(index: usize, row: &DiffRow, selected: usize) -> Vec<Line<'static>> {
    let mut result = vec![];
    if row.old_line.is_some() && (row.old_changed || row.new_line.is_none()) {
        result.push(unified_line(
            index,
            row.old_line,
            &row.old_text,
            &row.old_spans,
            '-',
            true,
            selected,
        ));
    }
    if row.new_line.is_some() {
        result.push(unified_line(
            index,
            row.new_line,
            &row.new_text,
            &row.new_spans,
            if row.new_changed { '+' } else { ' ' },
            false,
            selected,
        ));
    }
    result
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
            vec![
                RenderedDiffLine::Row(1),
                RenderedDiffLine::Row(2),
                RenderedDiffLine::Row(3),
                RenderedDiffLine::Row(4),
                RenderedDiffLine::Row(5),
                RenderedDiffLine::Row(6),
                RenderedDiffLine::Row(7),
                RenderedDiffLine::Separator,
                RenderedDiffLine::Row(16),
                RenderedDiffLine::Row(17),
                RenderedDiffLine::Row(18),
                RenderedDiffLine::Row(19),
                RenderedDiffLine::Row(20),
                RenderedDiffLine::Row(21),
                RenderedDiffLine::Row(22),
            ]
        );
    }
}
