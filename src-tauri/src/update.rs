//! GitHub Releases update check (silent on failure / timeout / no newer tag).

use serde::{Deserialize, Serialize};

const REPO_LATEST: &str = "https://api.github.com/repos/Sakyvo/plot/releases/latest";
const TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub tag: String,
    pub body: String,
    pub download_url: String,
    pub html_url: String,
    pub current_version: String,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Strip leading `v`/`V` and parse `major.minor.patch` (extra pre-release ignored).
pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let s = raw.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_semver(remote), parse_semver(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

pub fn pick_download_url(assets: &[(String, String)], html_url: &str) -> String {
    assets
        .iter()
        .find(|(name, _)| name.to_ascii_lowercase().ends_with(".exe"))
        .map(|(_, url)| url.clone())
        .unwrap_or_else(|| html_url.to_string())
}

fn update_from_release(release: &GhRelease, current: &str) -> Option<UpdateInfo> {
    if !is_newer(&release.tag_name, current) {
        return None;
    }
    let version = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let assets: Vec<(String, String)> = release
        .assets
        .iter()
        .map(|a| (a.name.clone(), a.browser_download_url.clone()))
        .collect();
    Some(UpdateInfo {
        version,
        tag: release.tag_name.clone(),
        body: release.body.clone().unwrap_or_default(),
        download_url: pick_download_url(&assets, &release.html_url),
        html_url: release.html_url.clone(),
        current_version: current.to_string(),
    })
}

/// Fetch latest release; `Ok(None)` = no update / no release / soft failures.
/// Hard transport errors still return `Err` so the caller can log; UI treats both as silent.
pub fn check_latest(current_version: &str) -> Result<Option<UpdateInfo>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build();
    let resp = match agent
        .get(REPO_LATEST)
        .set("User-Agent", "Plot")
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if resp.status() == 404 {
        return Ok(None);
    }
    if resp.status() >= 400 {
        return Err(format!("GitHub API HTTP {}", resp.status()));
    }
    let release: GhRelease = resp.into_json().map_err(|e| e.to_string())?;
    Ok(update_from_release(&release, current_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parses_and_strips_v() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("2.0"), Some((2, 0, 0)));
    }

    #[test]
    fn newer_only_when_remote_greater() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn picks_exe_asset_over_html() {
        let assets = vec![
            ("notes.txt".into(), "https://x/notes".into()),
            ("plot.exe".into(), "https://x/plot.exe".into()),
        ];
        assert_eq!(
            pick_download_url(&assets, "https://x/rel"),
            "https://x/plot.exe"
        );
        assert_eq!(pick_download_url(&[], "https://x/rel"), "https://x/rel");
    }

    #[test]
    fn update_from_release_none_when_not_newer() {
        let r = GhRelease {
            tag_name: "v0.1.0".into(),
            body: Some("notes".into()),
            html_url: "https://github.com/Sakyvo/plot/releases/tag/v0.1.0".into(),
            assets: vec![],
        };
        assert!(update_from_release(&r, "0.1.0").is_none());
    }

    #[test]
    fn update_from_release_some_when_newer() {
        let r = GhRelease {
            tag_name: "v0.2.0".into(),
            body: Some("## Fixed\n- foo".into()),
            html_url: "https://github.com/Sakyvo/plot/releases/tag/v0.2.0".into(),
            assets: vec![GhAsset {
                name: "plot.exe".into(),
                browser_download_url: "https://github.com/.../plot.exe".into(),
            }],
        };
        let u = update_from_release(&r, "0.1.0").unwrap();
        assert_eq!(u.version, "0.2.0");
        assert_eq!(u.download_url, "https://github.com/.../plot.exe");
        assert!(u.body.contains("Fixed"));
    }
}
