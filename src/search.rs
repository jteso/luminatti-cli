//! Fuzzy matching for the changed-file picker.
use crate::git::FileItem;

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

pub(crate) fn fuzzy_file_indices(files: &[FileItem], query: &str) -> Vec<usize> {
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
}
