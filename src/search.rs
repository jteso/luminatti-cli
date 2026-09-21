//! Fuzzy matching for the changed-file picker.
use crate::git::FileItem;

pub(crate) fn fuzzy_file_indices(files: &[FileItem], query: &str) -> Vec<usize> {
    ranked_matches(files, query)
        .into_iter()
        .map(|(index, _)| index)
        .collect()
}

/// Ranked `(file index, matched character positions)` pairs for the query.
pub(crate) fn ranked_matches(files: &[FileItem], query: &str) -> Vec<(usize, Vec<usize>)> {
    if query.is_empty() {
        return (0..files.len()).map(|index| (index, Vec::new())).collect();
    }
    let mut matches = files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            fuzzy_match_positions(&file.path, query)
                .map(|(score, positions)| (index, score, positions))
        })
        .collect::<Vec<_>>();
    matches.sort_by(
        |(left_index, left_score, _), (right_index, right_score, _)| {
            right_score
                .cmp(left_score)
                .then_with(|| {
                    files[*left_index]
                        .path
                        .len()
                        .cmp(&files[*right_index].path.len())
                })
                .then_with(|| files[*left_index].path.cmp(&files[*right_index].path))
        },
    );
    matches
        .into_iter()
        .map(|(index, _, positions)| (index, positions))
        .collect()
}

/// Scores subsequence matches and collects matched character indices.
fn fuzzy_match_positions(path: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let candidate = path.to_lowercase();
    let mut search_from = 0;
    let mut previous = None;
    let mut score = 0;
    let mut positions = Vec::new();
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
        positions.push(index);
        previous = Some(index);
        search_from = index + needle.len_utf8();
    }
    if candidate.contains(&query.to_lowercase()) {
        score += 24;
    }
    Some((score, positions))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn ranked_matches_reports_matched_character_positions() {
        let files = vec![FileItem {
            path: "src/main.rs".into(),
            status: " M".into(),
        }];

        assert!(
            ranked_matches(&files, "")
                .into_iter()
                .all(|(_, positions)| positions.is_empty())
        );
        let positions = ranked_matches(&files, "mn").remove(0).1;
        assert_eq!(positions, vec![4, 7]);
    }
}
