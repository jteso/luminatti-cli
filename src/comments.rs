//! Local review notes and supported agent comment formats.
use crate::storage::store_version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewComment {
    #[serde(default = "new_id")]
    pub(crate) id: String,
    pub(crate) file_path: String,
    #[serde(default)]
    pub(crate) old_line: Option<u32>,
    #[serde(default)]
    pub(crate) new_line: Option<u32>,
    #[serde(default)]
    pub(crate) hunk: Option<u32>,
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) rationale: Option<String>,
    #[serde(default)]
    pub(crate) author: Option<String>,
    #[serde(default = "local_source")]
    pub(crate) source: String,
}
pub(crate) fn new_id() -> String {
    format!("luminatti:{}", Uuid::new_v4())
}
fn local_source() -> String {
    "user".into()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CommentStore {
    #[serde(default = "store_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) comments: Vec<ReviewComment>,
}
impl Default for CommentStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            comments: vec![],
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
pub(crate) enum AgentCommentFile {
    #[default]
    Empty,
    Store {
        #[serde(default)]
        comments: Vec<ReviewComment>,
    },
    List(Vec<ReviewComment>),
}
pub(crate) fn remove_local_comment(
    store: &mut CommentStore,
    selected: usize,
) -> Option<ReviewComment> {
    (selected < store.comments.len()).then(|| store.comments.remove(selected))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_comments_accept_hunk_shape() {
        let parsed: AgentCommentFile = serde_json::from_str(
            r#"{"comments":[{"filePath":"a.rs","newLine":9,"summary":"test"}]}"#,
        )
        .unwrap();
        match parsed {
            AgentCommentFile::Store { comments } => assert_eq!(comments[0].new_line, Some(9)),
            _ => panic!("wrong shape"),
        }
    }

    #[test]
    fn selected_local_comment_can_be_removed_without_affecting_others() {
        let comment = |summary: &str| ReviewComment {
            id: new_id(),
            file_path: "src/main.rs".into(),
            old_line: None,
            new_line: Some(1),
            hunk: None,
            summary: summary.into(),
            rationale: None,
            author: None,
            source: "user".into(),
        };
        let mut store = CommentStore {
            version: store_version(),
            comments: vec![comment("first"), comment("second")],
        };

        let removed = remove_local_comment(&mut store, 0).unwrap();
        assert_eq!(removed.summary, "first");
        assert_eq!(store.comments.len(), 1);
        assert_eq!(store.comments[0].summary, "second");
        assert!(remove_local_comment(&mut store, 1).is_none());
    }
}
