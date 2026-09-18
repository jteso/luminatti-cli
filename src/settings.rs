use serde::{Deserialize, Serialize};

use crate::diff_view::DiffMode;

pub(crate) const DEFAULT_DIVIDER: u16 = 34;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ProjectSettings {
    version: u32,
    pub(crate) diff_mode: DiffMode,
    pub(crate) show_unchanged: bool,
    pub(crate) divider: u16,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            version: 1,
            diff_mode: DiffMode::SideBySide,
            show_unchanged: false,
            divider: DEFAULT_DIVIDER,
        }
    }
}

impl ProjectSettings {
    pub(crate) fn new(diff_mode: DiffMode, show_unchanged: bool, divider: u16) -> Self {
        Self {
            diff_mode,
            show_unchanged,
            divider,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_stable_defaults() {
        let settings: ProjectSettings = serde_json::from_str(r#"{"diffMode":"unified"}"#).unwrap();

        assert_eq!(settings.diff_mode, DiffMode::Unified);
        assert!(!settings.show_unchanged);
        assert_eq!(settings.divider, DEFAULT_DIVIDER);
    }

    #[test]
    fn settings_round_trip_as_project_metadata() {
        let settings = ProjectSettings::new(DiffMode::Unified, true, 42);
        let json = serde_json::to_string(&settings).unwrap();
        let restored: ProjectSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, settings);
        assert!(json.contains(r#""diffMode":"unified""#));
    }
}
