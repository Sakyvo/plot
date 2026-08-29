use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_path: Option<String>,
    /// Absent means false: startup never auto-scans unless explicitly enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_scan_on_start: Option<bool>,
}

/// Missing or corrupt files fall back to defaults — settings are never fatal.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load(&tmp.path().join("nope/plot.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn corrupt_file_loads_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("plot.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), Settings::default());
    }

    #[test]
    fn save_then_load_roundtrips_and_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("cfg/dir/plot.json");
        let s = Settings {
            language: Some("zh-TW".into()),
            custom_path: Some("D:\\mc\\resourcepacks".into()),
            auto_scan_on_start: Some(true),
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn unset_keys_are_omitted_from_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("plot.json");
        save(
            &p,
            &Settings {
                language: Some("en".into()),
                custom_path: None,
                auto_scan_on_start: None,
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("custom_path"));
        assert!(!text.contains("auto_scan_on_start"));
    }
}
