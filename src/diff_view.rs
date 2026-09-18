use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::diff::{DiffRow, DiffSpan, StructuralChange, visible_diff_indices};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffMode {
    SideBySide,
    Unified,
}

pub(crate) fn rendered_diff_indices(
    rows: &[DiffRow],
    show_unchanged: bool,
    mode: DiffMode,
) -> Vec<usize> {
    visible_diff_indices(rows, show_unchanged)
        .into_iter()
        .flat_map(|index| {
            let line_count = if mode == DiffMode::Unified {
                unified_lines(index, &rows[index], usize::MAX).len()
            } else {
                1
            };
            std::iter::repeat_n(index, line_count)
        })
        .collect()
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
        .unwrap_or_else(|| "      ".into());
    let selected = index == selected && line.is_some();
    let prefix_style = selected_style(changed_style(changed, deletion), selected);
    let mut rendered = vec![Span::styled(
        prefix,
        prefix_style.add_modifier(Modifier::DIM),
    )];
    rendered.extend(styled_source_spans(
        text, spans, changed, deletion, selected,
    ));
    Line::from(rendered)
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
        format!("{}{:>5} ", marker, line.unwrap_or(0)),
        prefix_style.add_modifier(Modifier::DIM),
    )];
    rendered.extend(styled_source_spans(
        text, spans, changed, deletion, selected,
    ));
    Line::from(rendered)
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
        style.bg(Color::DarkGray)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::StructuralHighlight;

    #[test]
    fn missing_split_side_is_not_selected() {
        let missing = side_line(0, None, "", &[], false, true, 0);
        assert!(
            missing
                .spans
                .iter()
                .all(|span| span.style.bg != Some(Color::DarkGray))
        );

        let present = side_line(0, Some(1), "added", &[], true, false, 0);
        assert!(
            present
                .spans
                .iter()
                .all(|span| span.style.bg == Some(Color::DarkGray))
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
}
