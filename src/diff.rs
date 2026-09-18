use std::path::Path;

pub(crate) use difftastic::{StructuralChange, StructuralHighlight};
use difftastic::{StructuralSpan, structural_diff};
use similar::{ChangeTag, TextDiff};

#[derive(Clone, Debug)]
pub(crate) struct DiffDocument {
    pub(crate) language: String,
    pub(crate) has_syntactic_changes: bool,
    pub(crate) rows: Vec<DiffRow>,
}

#[derive(Clone, Debug)]
pub(crate) struct DiffRow {
    pub(crate) old_line: Option<u32>,
    pub(crate) new_line: Option<u32>,
    pub(crate) old_text: String,
    pub(crate) new_text: String,
    pub(crate) old_changed: bool,
    pub(crate) new_changed: bool,
    pub(crate) old_spans: Vec<DiffSpan>,
    pub(crate) new_spans: Vec<DiffSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) change: StructuralChange,
    pub(crate) highlight: StructuralHighlight,
}

impl DiffRow {
    pub(crate) fn is_changed(&self) -> bool {
        self.old_changed || self.new_changed || self.old_line.is_none() || self.new_line.is_none()
    }
}

pub(crate) fn structural_diff_document(
    display_path: &Path,
    before: &str,
    after: &str,
) -> DiffDocument {
    let structural = structural_diff(display_path, before, after);
    let old_lines = source_lines(before);
    let new_lines = source_lines(after);
    let rows = structural
        .lines
        .into_iter()
        .filter_map(|pair| {
            // Difftastic internally keeps the empty item after a terminal
            // newline. It is useful for newline diagnostics, but it is not a
            // displayable source line in this TUI.
            let old_line = pair
                .old_line
                .filter(|line| (*line as usize) <= old_lines.len());
            let new_line = pair
                .new_line
                .filter(|line| (*line as usize) <= new_lines.len());
            (old_line.is_some() || new_line.is_some()).then(|| DiffRow {
                old_line,
                new_line,
                old_text: line_text(&old_lines, old_line),
                new_text: line_text(&new_lines, new_line),
                old_changed: pair.old_changed,
                new_changed: pair.new_changed,
                old_spans: if old_line.is_some() {
                    pair.old_spans.into_iter().map(DiffSpan::from).collect()
                } else {
                    vec![]
                },
                new_spans: if new_line.is_some() {
                    pair.new_spans.into_iter().map(DiffSpan::from).collect()
                } else {
                    vec![]
                },
            })
        })
        .collect();

    DiffDocument {
        language: structural.language,
        has_syntactic_changes: structural.has_syntactic_changes,
        rows,
    }
}

pub(crate) fn line_diff_document(before: &str, after: &str) -> DiffDocument {
    let diff = TextDiff::from_lines(before, after);
    let mut rows = vec![];
    let mut old_line = 0u32;
    let mut new_line = 0u32;
    let mut deleted = vec![];
    let mut inserted = vec![];

    for change in diff.iter_all_changes() {
        let text = source_line(change.value()).to_owned();
        match change.tag() {
            ChangeTag::Delete => {
                old_line += 1;
                deleted.push((old_line, text));
            }
            ChangeTag::Insert => {
                new_line += 1;
                inserted.push((new_line, text));
            }
            ChangeTag::Equal => {
                flush_line_changes(&mut rows, &mut deleted, &mut inserted);
                old_line += 1;
                new_line += 1;
                rows.push(DiffRow {
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    old_text: text.clone(),
                    new_text: text.clone(),
                    old_changed: false,
                    new_changed: false,
                    old_spans: whole_line_span(&text, StructuralChange::Unchanged),
                    new_spans: whole_line_span(&text, StructuralChange::Unchanged),
                });
            }
        }
    }
    flush_line_changes(&mut rows, &mut deleted, &mut inserted);

    DiffDocument {
        language: String::new(),
        has_syntactic_changes: rows.iter().any(DiffRow::is_changed),
        rows,
    }
}

fn source_line(value: &str) -> &str {
    let value = value.strip_suffix('\n').unwrap_or(value);
    value.strip_suffix('\r').unwrap_or(value)
}

fn flush_line_changes(
    rows: &mut Vec<DiffRow>,
    deleted: &mut Vec<(u32, String)>,
    inserted: &mut Vec<(u32, String)>,
) {
    let changed_count = deleted.len().max(inserted.len());
    for index in 0..changed_count {
        let old = deleted.get(index);
        let new = inserted.get(index);
        let (old_spans, new_spans) = match (old, new) {
            (Some((_, old_text)), Some((_, new_text))) => word_diff_spans(old_text, new_text),
            (Some((_, old_text)), None) => {
                (whole_line_span(old_text, StructuralChange::Novel), vec![])
            }
            (None, Some((_, new_text))) => {
                (vec![], whole_line_span(new_text, StructuralChange::Novel))
            }
            (None, None) => unreachable!("change block contains at least one line"),
        };
        rows.push(DiffRow {
            old_line: old.map(|(line, _)| *line),
            new_line: new.map(|(line, _)| *line),
            old_text: old.map(|(_, text)| text.clone()).unwrap_or_default(),
            new_text: new.map(|(_, text)| text.clone()).unwrap_or_default(),
            old_changed: old.is_some(),
            new_changed: new.is_some(),
            old_spans,
            new_spans,
        });
    }
    deleted.clear();
    inserted.clear();
}

fn word_diff_spans(old: &str, new: &str) -> (Vec<DiffSpan>, Vec<DiffSpan>) {
    let old_tokens = lexical_tokens(old);
    let new_tokens = lexical_tokens(new);
    let diff = TextDiff::from_slices(&old_tokens, &new_tokens);
    let mut old_spans = vec![];
    let mut new_spans = vec![];
    let mut old_cursor = 0;
    let mut new_cursor = 0;

    for change in diff.iter_all_changes() {
        let length = change.value().len();
        match change.tag() {
            ChangeTag::Equal => {
                old_spans.push(diff_span(
                    old_cursor,
                    old_cursor + length,
                    StructuralChange::Unchanged,
                ));
                new_spans.push(diff_span(
                    new_cursor,
                    new_cursor + length,
                    StructuralChange::Unchanged,
                ));
                old_cursor += length;
                new_cursor += length;
            }
            ChangeTag::Delete => {
                old_spans.push(diff_span(
                    old_cursor,
                    old_cursor + length,
                    StructuralChange::NovelWord,
                ));
                old_cursor += length;
            }
            ChangeTag::Insert => {
                new_spans.push(diff_span(
                    new_cursor,
                    new_cursor + length,
                    StructuralChange::NovelWord,
                ));
                new_cursor += length;
            }
        }
    }
    (old_spans, new_spans)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Word,
    Whitespace,
    Punctuation,
}

fn lexical_tokens(text: &str) -> Vec<&str> {
    let mut tokens = vec![];
    let mut start = 0;
    let mut current = None;

    for (index, character) in text.char_indices() {
        let class = if character.is_alphanumeric() || character == '_' {
            TokenClass::Word
        } else if character.is_whitespace() {
            TokenClass::Whitespace
        } else {
            TokenClass::Punctuation
        };
        if current.is_some_and(|current| current != class) {
            tokens.push(&text[start..index]);
            start = index;
        }
        current = Some(class);
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

fn whole_line_span(text: &str, change: StructuralChange) -> Vec<DiffSpan> {
    (!text.is_empty())
        .then(|| diff_span(0, text.len(), change))
        .into_iter()
        .collect()
}

fn diff_span(start: usize, end: usize, change: StructuralChange) -> DiffSpan {
    DiffSpan {
        start,
        end,
        change,
        highlight: StructuralHighlight::Normal,
    }
}

fn source_lines(source: &str) -> Vec<&str> {
    source.lines().collect()
}

fn line_text(lines: &[&str], line: Option<u32>) -> String {
    line.and_then(|line| lines.get(line.saturating_sub(1) as usize))
        .copied()
        .unwrap_or_default()
        .to_owned()
}

impl From<StructuralSpan> for DiffSpan {
    fn from(span: StructuralSpan) -> Self {
        Self {
            start: span.start as usize,
            end: span.end as usize,
            change: span.change,
            highlight: span.highlight,
        }
    }
}

pub(crate) fn visible_diff_indices(rows: &[DiffRow], show_unchanged: bool) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| (show_unchanged || row.is_changed()).then_some(index))
        .collect()
}

pub(crate) fn changed_row_indices(rows: &[DiffRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| row.is_changed().then_some(index))
        .collect()
}

pub(crate) fn adjacent_changed_row(
    rows: &[DiffRow],
    selected: usize,
    forward: bool,
) -> Option<usize> {
    let changed = changed_row_indices(rows);
    if forward {
        changed.into_iter().find(|index| *index > selected)
    } else {
        changed.into_iter().rfind(|index| *index < selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed_text<'a>(text: &'a str, spans: &[DiffSpan]) -> Vec<&'a str> {
        spans
            .iter()
            .filter(|span| span.change.is_changed())
            .filter_map(|span| text.get(span.start..span.end))
            .collect()
    }

    #[test]
    fn typescript_diff_preserves_token_level_changes() {
        let document = structural_diff_document(
            Path::new("employee.ts"),
            "const employee = oldEmployee;\n",
            "const employee = newEmployee;\n",
        );

        assert_eq!(document.language, "TypeScript");
        assert!(document.has_syntactic_changes);
        let row = document.rows.iter().find(|row| row.is_changed()).unwrap();
        assert_eq!(row.old_line, Some(1));
        assert_eq!(row.new_line, Some(1));
        assert_eq!(
            changed_text(&row.old_text, &row.old_spans),
            vec!["oldEmployee"]
        );
        assert_eq!(
            changed_text(&row.new_text, &row.new_spans),
            vec!["newEmployee"]
        );
    }

    #[test]
    fn structural_alignment_keeps_added_object_fields_on_the_new_side() {
        let document = structural_diff_document(
            Path::new("employee.ts"),
            "const value = {\n  name: employee.name,\n};\n",
            "const value = {\n  name: employee.name,\n  city: employee.city,\n};\n",
        );

        let added = document
            .rows
            .iter()
            .find(|row| row.new_text.contains("city:"))
            .unwrap();
        assert_eq!(added.old_line, None);
        assert_eq!(added.new_line, Some(3));
        assert!(added.new_changed);
    }

    #[test]
    fn multiline_signature_and_expression_edits_keep_structural_correspondence() {
        let document = structural_diff_document(
            Path::new("payroll.ts"),
            "export function mapEmployee(employee: PreparedEmployee): PayrollEmployee {\n  const values = {\n    employeeCountry: resolve(employee.values.employeeCountry),\n  };\n}\n",
            "export function mapEmployee(\n  employee: PreparedEmployee,\n  enrichment: PayrollEnrichment = {},\n): PayrollEmployee {\n  const merged = { ...employee.values, ...enrichment };\n  const values = {\n    employeeCountry: resolve(merged.employeeCountry),\n  };\n}\n",
        );

        let signature = document
            .rows
            .iter()
            .find(|row| row.old_text.starts_with("export function"))
            .unwrap();
        assert!(signature.new_text.starts_with("export function"));
        assert!(
            signature
                .old_spans
                .iter()
                .any(|span| span.change == StructuralChange::Unchanged)
        );

        let country = document
            .rows
            .iter()
            .find(|row| row.old_text.contains("employeeCountry:"))
            .unwrap();
        assert!(country.new_text.contains("employeeCountry:"));
        assert_eq!(
            changed_text(&country.new_text, &country.new_spans).concat(),
            "merged"
        );
    }

    #[test]
    fn unchanged_rows_can_be_included_or_hidden() {
        let rows =
            structural_diff_document(Path::new("example.rs"), "same\nold\n", "same\nnew\n").rows;
        assert_eq!(visible_diff_indices(&rows, false), vec![1]);
        assert_eq!(visible_diff_indices(&rows, true), vec![0, 1]);
    }

    #[test]
    fn changed_row_navigation_includes_adjacent_changes() {
        let rows = structural_diff_document(
            Path::new("example.txt"),
            "same\nold one\nold two\nbetween\nold three\nend\n",
            "same\nnew one\nnew two\nbetween\nnew three\nend\n",
        )
        .rows;

        let changed = changed_row_indices(&rows);
        assert!(changed.len() >= 3);
        assert_eq!(
            adjacent_changed_row(&rows, changed[0], true),
            Some(changed[1])
        );
        assert_eq!(
            adjacent_changed_row(&rows, changed[1], false),
            Some(changed[0])
        );
    }

    #[test]
    fn line_diff_pairs_physical_lines_and_marks_changed_words() {
        let document = line_diff_document(
            "const employee = oldEmployee;\nunchanged\n",
            "const employee = newEmployee;\nunchanged\n",
        );

        assert_eq!(document.rows.len(), 2);
        let changed = &document.rows[0];
        assert_eq!((changed.old_line, changed.new_line), (Some(1), Some(1)));
        assert_eq!(
            changed_text(&changed.old_text, &changed.old_spans),
            vec!["oldEmployee"]
        );
        assert_eq!(
            changed_text(&changed.new_text, &changed.new_spans),
            vec!["newEmployee"]
        );
    }

    #[test]
    fn line_diff_keeps_extra_insertions_on_the_new_side() {
        let document = line_diff_document("first\nlast\n", "first\nadded\nlast\n");
        let added = document
            .rows
            .iter()
            .find(|row| row.new_text == "added")
            .unwrap();

        assert_eq!(added.old_line, None);
        assert_eq!(added.new_line, Some(2));
        assert!(added.new_changed);
    }
}
