use serde::Serialize;
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

/// Containers nested deeper than this are not worth rescuing.
pub const MAX_NESTING_DEPTH: usize = 10;

/// OS/archiver junk that is never a pack and never worth surfacing.
pub const JUNK_NAMES: [&str; 4] = ["__macosx", ".ds_store", "thumbs.db", "desktop.ini"];

pub fn is_junk_name(name: &str) -> bool {
    JUNK_NAMES.contains(&name.to_ascii_lowercase().as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Ok,
    MissingDir,
    NoPacks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Normal,
    Nested,
    Folder,
    Bloated,
    Illegal,
    /// Structure is a normal zip; pack.mcmeta has Lunar-illegal escapes.
    LunarIllegal,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackEntry {
    pub name: String,
    pub category: Category,
    /// Machine-readable cause keys; the UI maps them to localized text.
    pub causes: Vec<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Counts {
    pub normal: usize,
    pub nested: usize,
    pub folder: usize,
    pub bloated: usize,
    pub illegal: usize,
    /// Packs tagged Lunar (pure or stacked). Independent of structure counts.
    pub lunar: usize,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub path: String,
    pub status: ScanStatus,
    pub entries: Vec<PackEntry>,
    pub counts: Counts,
}

#[derive(Debug, Default, Clone)]
pub struct ScanOptions {
    /// Absolute paths to hide from scanning (e.g. the running exe).
    pub exclude: Vec<std::path::PathBuf>,
}

#[derive(Debug)]
pub struct ScanProgress {
    pub name: String,
    /// Completed count so far (1-based), not a stable position.
    pub index: usize,
    pub total: usize,
}

pub fn scan(dir: &Path) -> ScanReport {
    scan_with(dir, &ScanOptions::default())
}

pub fn scan_with(dir: &Path, opts: &ScanOptions) -> ScanReport {
    scan_with_progress(dir, opts, &|_| {})
}

pub fn scan_with_progress(
    dir: &Path,
    opts: &ScanOptions,
    on_progress: &(dyn Fn(&ScanProgress) + Sync),
) -> ScanReport {
    use rayon::prelude::*;

    let path = dir.to_string_lossy().into_owned();
    if !dir.is_dir() {
        return ScanReport {
            path,
            status: ScanStatus::MissingDir,
            entries: Vec::new(),
            counts: Counts::default(),
        };
    }
    let excludes: Vec<std::path::PathBuf> = opts
        .exclude
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    let mut items: Vec<(String, std::path::PathBuf, bool)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if is_junk_name(&name) {
                continue;
            }
            let item_path = item.path();
            if name.eq_ignore_ascii_case("plot_temp") && item_path.is_dir() {
                continue;
            }
            if let Ok(canon) = item_path.canonicalize() {
                if excludes.contains(&canon) {
                    continue;
                }
            }
            let is_dir = item_path.is_dir();
            items.push((name, item_path, is_dir));
        }
    }
    let total = items.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let entries: Vec<PackEntry> = items
        .into_par_iter()
        .map(|(name, item_path, is_dir)| {
            let (category, causes) = if is_dir {
                classify_dir(&item_path)
            } else {
                classify_file(&item_path, &name)
            };
            let (category, causes) = apply_lunar_tag(category, causes, &item_path, is_dir);
            let size_bytes = if is_dir {
                dir_size(&item_path)
            } else {
                std::fs::metadata(&item_path).map(|m| m.len()).unwrap_or(0)
            };
            let index = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            on_progress(&ScanProgress {
                name: name.clone(),
                index,
                total,
            });
            PackEntry {
                name,
                category,
                causes,
                size_bytes,
            }
        })
        .collect();
    let mut counts = Counts::default();
    for e in &entries {
        match e.category {
            Category::Normal => counts.normal += 1,
            Category::Nested => counts.nested += 1,
            Category::Folder => counts.folder += 1,
            Category::Bloated => counts.bloated += 1,
            Category::Illegal => counts.illegal += 1,
            Category::LunarIllegal => {}
        }
        if e.category == Category::LunarIllegal
            || e.causes.iter().any(|c| c == "lunar_escape")
        {
            counts.lunar += 1;
        }
    }
    let status = if entries.is_empty() {
        ScanStatus::NoPacks
    } else {
        ScanStatus::Ok
    };
    ScanReport {
        path,
        status,
        entries,
        counts,
    }
}

/// Stack Lunar tag after structure classification. Illegal packs are never tagged.
/// Pure normal + escape → LunarIllegal; structure categories keep their primary class.
fn apply_lunar_tag(
    category: Category,
    mut causes: Vec<String>,
    path: &Path,
    is_dir: bool,
) -> (Category, Vec<String>) {
    if category == Category::Illegal {
        return (category, causes);
    }
    let Some(raw) = read_judgment_mcmeta(path, is_dir) else {
        return (category, causes);
    };
    if !mcmeta_has_lunar_escape(&raw) {
        return (category, causes);
    }
    if !causes.iter().any(|c| c == "lunar_escape") {
        causes.push("lunar_escape".into());
    }
    match category {
        Category::Normal => (Category::LunarIllegal, causes),
        other => (other, causes),
    }
}

/// Mcmeta used for Lunar judgment: root pack.mcmeta, else core-layer pack.mcmeta
/// inside nested containers (same layer that has assets).
fn read_judgment_mcmeta(path: &Path, is_dir: bool) -> Option<Vec<u8>> {
    if is_dir {
        let root = path.join("pack.mcmeta");
        if root.is_file() {
            return std::fs::read(root).ok();
        }
        return find_fs_core_mcmeta(path, 1);
    }
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    if let Ok(mut e) = archive.by_name("pack.mcmeta") {
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }
    let names: Vec<String> = archive.file_names().map(|n| n.to_string()).collect();
    for n in &names {
        if !n.ends_with("pack.mcmeta") {
            continue;
        }
        let prefix = n.trim_end_matches("pack.mcmeta");
        let assets = format!("{prefix}assets/");
        if !names.iter().any(|x| x.starts_with(&assets)) {
            continue;
        }
        if let Ok(mut e) = archive.by_name(n) {
            let mut buf = Vec::new();
            if e.read_to_end(&mut buf).is_ok() {
                return Some(buf);
            }
        }
    }
    None
}

fn find_fs_core_mcmeta(dir: &Path, child_depth: usize) -> Option<Vec<u8>> {
    if child_depth > MAX_NESTING_DEPTH {
        return None;
    }
    let rd = std::fs::read_dir(dir).ok()?;
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_junk_name(&name) {
            continue;
        }
        let p = item.path();
        if p.is_dir() {
            if p.join("assets").is_dir() {
                let mc = p.join("pack.mcmeta");
                if mc.is_file() {
                    return std::fs::read(mc).ok();
                }
            } else if let Some(b) = find_fs_core_mcmeta(&p, child_depth + 1) {
                return Some(b);
            }
        } else if let Ok(bytes) = std::fs::read(&p) {
            if bytes.len() >= 2 && &bytes[..2] == b"PK" {
                if let Some(b) = read_judgment_mcmeta(&p, false) {
                    return Some(b);
                }
            }
        }
    }
    None
}

/// The one mcmeta disease Lunar chokes on: an invalid JSON escape sequence.
/// § (literal or textual escape), BOM, control chars and non-UTF-8 encodings
/// are all tolerated by Lunar's parser and must not trigger.
fn mcmeta_has_lunar_escape(raw: &[u8]) -> bool {
    match std::str::from_utf8(raw) {
        // valid UTF-8: 0x5C is always a real backslash
        Ok(_) => scan_invalid_escape(raw, false),
        // unknown legacy encoding: only trust a backslash after an ASCII byte,
        // so a multi-byte character's trail byte can't be misread as an escape
        Err(_) => scan_invalid_escape(raw, true),
    }
}

fn scan_invalid_escape(bytes: &[u8], ascii_guard: bool) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || (ascii_guard && i > 0 && bytes[i - 1] >= 0x80) {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => i += 2,
            Some(b'u') => match bytes.get(i + 2..i + 6) {
                Some(h) if h.iter().all(u8::is_ascii_hexdigit) => i += 6,
                _ => return true,
            },
            _ => return true,
        }
    }
    false
}

/// Minimal Lunar repair: drop the backslash byte of every invalid escape,
/// keeping everything else — the following char, § codes, formatting,
/// encoding — byte-for-byte.
fn patch_lunar_escapes(raw: &[u8]) -> Vec<u8> {
    let ascii_guard = std::str::from_utf8(raw).is_err();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'\\' || (ascii_guard && i > 0 && raw[i - 1] >= 0x80) {
            out.push(raw[i]);
            i += 1;
            continue;
        }
        match raw.get(i + 1) {
            Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                out.extend_from_slice(&raw[i..i + 2]);
                i += 2;
            }
            Some(b'u') => match raw.get(i + 2..i + 6) {
                Some(h) if h.iter().all(u8::is_ascii_hexdigit) => {
                    out.extend_from_slice(&raw[i..i + 6]);
                    i += 6;
                }
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    out
}

/// Structural probe mirroring Lunar's leniencies: BOM and raw control chars
/// are tolerated (neutralized before the strict parse), broken structure is not.
fn parses_like_lunar(raw: &[u8]) -> bool {
    let body = raw.strip_prefix(b"\xef\xbb\xbf".as_slice()).unwrap_or(raw);
    let softened: Vec<u8> = body
        .iter()
        .map(|&b| if b < 0x20 { b' ' } else { b })
        .collect();
    serde_json::from_slice::<serde::de::IgnoredAny>(&softened).is_ok()
}

/// Every product's mcmeta passes through the Lunar patch; a file the patch
/// cannot save falls back to mcmeta rescue (a regenerated default).
fn product_mcmeta(raw: Vec<u8>, stem: &str) -> Vec<u8> {
    if !mcmeta_has_lunar_escape(&raw) {
        return raw;
    }
    let patched = patch_lunar_escapes(&raw);
    if parses_like_lunar(&patched) {
        patched
    } else {
        generated_mcmeta(stem).into_bytes()
    }
}

fn classify_dir(path: &Path) -> (Category, Vec<String>) {
    let facts = facts_from_dir(path);
    if facts.has_assets && facts.has_mcmeta {
        return (Category::Folder, facts.bloat_causes());
    }
    if facts.has_assets {
        return (Category::Nested, vec!["mcmeta_rescue".into()]);
    }
    let mut search = CoreSearch::default();
    search_fs(path, 1, &mut search);
    verdict_from_search(search)
}

/// Outcome of hunting for rescuable core layers inside a container.
#[derive(Default)]
struct CoreSearch {
    found: Vec<String>,
    hit_cap: bool,
}

fn verdict_from_search(search: CoreSearch) -> (Category, Vec<String>) {
    if !search.found.is_empty() {
        return (Category::Nested, vec!["nested_container".into()]);
    }
    if search.hit_cap {
        return (Category::Illegal, vec!["too_deep".into()]);
    }
    (Category::Illegal, vec!["no_core_found".into()])
}

fn file_stem_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

/// A layer counts as a rescuable core if it has an assets/ child —
/// pack.mcmeta is the one synthesizable core file.
fn search_fs(dir: &Path, child_depth: usize, out: &mut CoreSearch) {
    if child_depth > MAX_NESTING_DEPTH {
        out.hit_cap = true;
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_junk_name(&name) {
            continue;
        }
        let p = item.path();
        if p.is_dir() {
            if p.join("assets").is_dir() {
                out.found.push(name);
            } else {
                search_fs(&p, child_depth + 1, out);
            }
        } else if let Ok(bytes) = std::fs::read(&p) {
            if bytes.len() >= 2 && &bytes[..2] == b"PK" {
                if let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(bytes)) {
                    let inames: Vec<String> =
                        inner.file_names().map(|n| n.to_string()).collect();
                    if inames.iter().any(|n| n.starts_with("assets/")) {
                        out.found.push(file_stem_of(&name));
                    } else {
                        search_zip(&mut inner, &inames, "", child_depth + 1, out);
                    }
                }
            }
        }
    }
}

fn zip_child_dirs(names: &[String], prefix: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for n in names {
        if let Some(rest) = n.strip_prefix(prefix) {
            if let Some(idx) = rest.find('/') {
                let d = &rest[..idx];
                if !d.is_empty() && !is_junk_name(d) {
                    out.insert(d.to_string());
                }
            }
        }
    }
    out
}

fn zip_child_files(names: &[String], prefix: &str) -> Vec<String> {
    names
        .iter()
        .filter_map(|n| n.strip_prefix(prefix))
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
        .filter(|rest| !is_junk_name(rest))
        .map(|rest| rest.to_string())
        .collect()
}

fn search_zip<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
    child_depth: usize,
    out: &mut CoreSearch,
) {
    if child_depth > MAX_NESTING_DEPTH {
        out.hit_cap = true;
        return;
    }
    for d in zip_child_dirs(names, prefix) {
        let layer = format!("{prefix}{d}/");
        let has_assets = names.iter().any(|n| n.starts_with(&format!("{layer}assets/")));
        if has_assets {
            out.found.push(d);
        } else {
            search_zip(archive, names, &layer, child_depth + 1, out);
        }
    }
    for f in zip_child_files(names, prefix) {
        let full = format!("{prefix}{f}");
        let mut bytes = Vec::new();
        {
            let Ok(mut entry) = archive.by_name(&full) else {
                continue;
            };
            if entry.read_to_end(&mut bytes).is_err() {
                continue;
            }
        }
        if bytes.len() < 2 || &bytes[..2] != b"PK" {
            continue;
        }
        let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(bytes)) else {
            continue;
        };
        let inames: Vec<String> = inner.file_names().map(|n| n.to_string()).collect();
        if inames.iter().any(|n| n.starts_with("assets/")) {
            out.found.push(file_stem_of(&f));
        } else {
            search_zip(&mut inner, &inames, "", child_depth + 1, out);
        }
    }
}

/// What the root layer of a candidate pack looks like, however it is stored.
struct RootFacts {
    has_assets: bool,
    has_mcmeta: bool,
    mcmeta_rescuable: bool,
    png_rescuable: bool,
    has_extras: bool,
    dead_path: bool,
}

impl RootFacts {
    fn bloat_causes(&self) -> Vec<String> {
        let mut causes = Vec::new();
        if self.has_extras {
            causes.push("root_extras".into());
        }
        if self.dead_path {
            causes.push("dead_path".into());
        }
        if self.png_rescuable {
            causes.push("png_rescue".into());
        }
        causes
    }
}

fn facts_from_zip_names(names: &[String]) -> RootFacts {
    let has_assets = names.iter().any(|n| n.starts_with("assets/"));
    let has_mcmeta = names.iter().any(|n| n == "pack.mcmeta");
    let root_files: Vec<&str> = names
        .iter()
        .filter(|n| !n.contains('/'))
        .map(|s| s.as_str())
        .collect();
    let mcmeta_rescuable = root_files.iter().any(|f| is_mcmeta_variant(f));
    let png_rescuable = root_files.iter().any(|f| is_png_variant(f));

    let mut roots = BTreeSet::new();
    for n in names {
        let first = n.split('/').next().unwrap_or("");
        if !first.is_empty() {
            roots.insert(first.to_string());
        }
    }
    let has_extras = roots.iter().any(|r| {
        r.as_str() != "assets"
            && r.as_str() != "pack.mcmeta"
            && r.as_str() != "pack.png"
            && !is_junk_name(r)
            && !is_mcmeta_variant(r)
            && !is_png_variant(r)
    });
    let dead_path = names.iter().any(|n| is_dead_path(n));
    RootFacts {
        has_assets,
        has_mcmeta,
        mcmeta_rescuable,
        png_rescuable,
        has_extras,
        dead_path,
    }
}

fn facts_from_dir(path: &Path) -> RootFacts {
    let mut facts = RootFacts {
        has_assets: false,
        has_mcmeta: false,
        mcmeta_rescuable: false,
        png_rescuable: false,
        has_extras: false,
        dead_path: false,
    };
    let Ok(rd) = std::fs::read_dir(path) else {
        return facts;
    };
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_junk_name(&name) {
            continue;
        }
        let is_dir = item.path().is_dir();
        match name.as_str() {
            "assets" if is_dir => facts.has_assets = true,
            "pack.mcmeta" if !is_dir => facts.has_mcmeta = true,
            "pack.png" if !is_dir => {}
            _ if !is_dir && is_mcmeta_variant(&name) => facts.mcmeta_rescuable = true,
            _ if !is_dir && is_png_variant(&name) => facts.png_rescuable = true,
            _ => facts.has_extras = true,
        }
    }
    if facts.has_assets {
        // Targeted probe: assets/<ns>/records is the only dead path so far.
        if let Ok(ns_dirs) = std::fs::read_dir(path.join("assets")) {
            for ns in ns_dirs.flatten() {
                if ns.path().join("records").is_dir() {
                    facts.dead_path = true;
                    break;
                }
            }
        }
    }
    facts
}

// ---------- processing ----------

#[derive(Debug, Clone)]
pub struct ProcessOptions {
    pub resourcepacks: std::path::PathBuf,
    pub plot_temp: std::path::PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackOutcome {
    pub original_name: String,
    /// Machine key: moved_to_illegal | skipped_unsupported | converted | failed | skipped_locked
    pub action: String,
    pub products: Vec<String>,
    pub causes: Vec<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcessReport {
    pub outcomes: Vec<PackOutcome>,
}

#[derive(Debug)]
pub struct ProgressEvent {
    pub name: String,
    pub index: usize,
    pub total: usize,
}

#[derive(Debug)]
pub enum ProcessError {
    PlotTempNotWritable(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::PlotTempNotWritable(e) => {
                write!(f, "plot_temp is not writable: {e}")
            }
        }
    }
}

impl std::error::Error for ProcessError {}

pub fn process(opts: &ProcessOptions) -> Result<ProcessReport, ProcessError> {
    process_with_progress(opts, &mut |_| {})
}

pub fn process_with_progress(
    opts: &ProcessOptions,
    on_progress: &mut dyn FnMut(&ProgressEvent),
) -> Result<ProcessReport, ProcessError> {
    let illegal_dir = opts.plot_temp.join("illegal_packs");
    let problematic_dir = opts.plot_temp.join("problematic_packs");
    std::fs::create_dir_all(&illegal_dir)
        .map_err(|e| ProcessError::PlotTempNotWritable(e.to_string()))?;
    std::fs::create_dir_all(&problematic_dir)
        .map_err(|e| ProcessError::PlotTempNotWritable(e.to_string()))?;

    let report = scan(&opts.resourcepacks);
    let work: Vec<&PackEntry> = report
        .entries
        .iter()
        .filter(|e| {
            e.category != Category::Normal
                || e.causes.iter().any(|c| c == "lunar_escape")
        })
        .collect();
    let total = work.len();
    let mut outcomes = Vec::new();
    for (index, entry) in work.into_iter().enumerate() {
        on_progress(&ProgressEvent {
            name: entry.name.clone(),
            index,
            total,
        });
        let src = opts.resourcepacks.join(&entry.name);
        let outcome = match entry.category {
            Category::Illegal => match move_into(&src, &illegal_dir) {
                Ok(_) => PackOutcome {
                    original_name: entry.name.clone(),
                    action: "moved_to_illegal".into(),
                    products: Vec::new(),
                    causes: entry.causes.clone(),
                    detail: None,
                },
                Err(e) => {
                    let action = if e.raw_os_error() == Some(LOCKED_OS_ERROR) {
                        "skipped_locked"
                    } else {
                        "failed"
                    };
                    PackOutcome {
                        original_name: entry.name.clone(),
                        action: action.into(),
                        products: Vec::new(),
                        causes: entry.causes.clone(),
                        detail: Some(e.to_string()),
                    }
                }
            },
            Category::Nested if entry.causes.iter().any(|c| c == "nested_container") => {
                convert_nested_pack(
                    &src,
                    entry,
                    &problematic_dir,
                    &opts.resourcepacks,
                    &opts.plot_temp,
                )
            }
            _ => convert_pack(&src, entry, &problematic_dir, &opts.resourcepacks),
        };
        outcomes.push(outcome);
    }
    Ok(ProcessReport { outcomes })
}

const LOCKED_OS_ERROR: i32 = 32; // ERROR_SHARING_VIOLATION

/// Write-conflict precheck: which of `names` cannot be safely renamed right
/// now. File-form packs only — folder packs never join (MC opens their files
/// on demand and holds no lasting handle); runtime skipped_locked covers them.
pub fn probe_locked(resourcepacks: &Path, names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| file_is_locked(&resourcepacks.join(n.as_str())))
        .cloned()
        .collect()
}

#[cfg(windows)]
fn file_is_locked(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    if !path.is_file() {
        return false;
    }
    const DELETE: u32 = 0x0001_0000;
    const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
    match std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(SHARE_ALL)
        .open(path)
    {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(LOCKED_OS_ERROR),
    }
}

#[cfg(not(windows))]
fn file_is_locked(_path: &Path) -> bool {
    false
}

fn locked_or_failed_outcome(entry: &PackEntry, e: &std::io::Error) -> PackOutcome {
    let action = if e.raw_os_error() == Some(LOCKED_OS_ERROR) {
        "skipped_locked"
    } else {
        "failed"
    };
    PackOutcome {
        original_name: entry.name.clone(),
        action: action.into(),
        products: Vec::new(),
        causes: entry.causes.clone(),
        detail: Some(e.to_string()),
    }
}

/// Container unwrap: move the original out, extract every rescuable core
/// layer (collections split), and rebuild each as a clean zip named after
/// its inner container.
fn convert_nested_pack(
    src: &Path,
    entry: &PackEntry,
    problematic_dir: &Path,
    resourcepacks: &Path,
    plot_temp: &Path,
) -> PackOutcome {
    let moved = match move_into(src, problematic_dir) {
        Ok(p) => p,
        Err(e) => return locked_or_failed_outcome(entry, &e),
    };
    let work_root = plot_temp.join(".work");
    let _ = std::fs::create_dir_all(&work_root);
    let work = unique_target(&work_root, &format!("{}.work", file_stem_of(&entry.name)));
    let result = unwrap_layers(&moved, &entry.name, &work, resourcepacks);
    let _ = std::fs::remove_dir_all(&work);
    match result {
        Ok(products) => PackOutcome {
            original_name: entry.name.clone(),
            action: "converted".into(),
            products,
            causes: entry.causes.clone(),
            detail: None,
        },
        Err(e) => PackOutcome {
            original_name: entry.name.clone(),
            action: "failed".into(),
            products: Vec::new(),
            causes: entry.causes.clone(),
            detail: Some(e.to_string()),
        },
    }
}

fn unwrap_layers(
    moved: &Path,
    original_name: &str,
    work: &Path,
    resourcepacks: &Path,
) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(work)?;
    let root: std::path::PathBuf = if moved.is_dir() {
        moved.to_path_buf()
    } else {
        let dst = work.join("_root");
        extract_zip_to_dir(moved, &dst)?;
        dst
    };
    let mut layers: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut hit_cap = false;
    collect_core_layers(&root, 1, work, &mut layers, &mut hit_cap)?;
    if layers.is_empty() {
        return Err(std::io::Error::other(format!(
            "no rescuable layer found inside {original_name}"
        )));
    }
    let mut products = Vec::new();
    for (name, dir) in layers {
        let sanitized = sanitize_windows_name(&name);
        products.push(build_root_fixed_product(&dir, &sanitized, resourcepacks)?);
    }
    Ok(products)
}

/// Walks a container tree, extracting nested zips into `work`, and returns
/// every layer that has an assets/ child (the rescuable-core criterion).
fn collect_core_layers(
    dir: &Path,
    child_depth: usize,
    work: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
    hit_cap: &mut bool,
) -> std::io::Result<()> {
    if child_depth > MAX_NESTING_DEPTH {
        *hit_cap = true;
        return Ok(());
    }
    for item in std::fs::read_dir(dir)?.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_junk_name(&name) {
            continue;
        }
        let p = item.path();
        if p.is_dir() {
            if p.join("assets").is_dir() {
                out.push((name, p));
            } else {
                collect_core_layers(&p, child_depth + 1, work, out, hit_cap)?;
            }
        } else {
            let mut head = [0u8; 2];
            let is_zip = std::fs::File::open(&p)
                .and_then(|mut f| f.read_exact(&mut head))
                .map(|_| &head == b"PK")
                .unwrap_or(false);
            if !is_zip {
                continue;
            }
            let stem = file_stem_of(&name);
            let dst = unique_target(work, &format!("{}.x", sanitize_windows_name(&stem)));
            extract_zip_to_dir(&p, &dst)?;
            if dst.join("assets").is_dir() {
                out.push((stem, dst));
            } else {
                collect_core_layers(&dst, child_depth + 1, work, out, hit_cap)?;
            }
        }
    }
    Ok(())
}

/// UTF-8 first, GBK fallback — how Chinese archivers write entry names
/// without the UTF-8 flag.
fn decode_entry_name(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    let (cow, _, _) = encoding_rs::GBK.decode(raw);
    cow.into_owned()
}

/// Makes a string safe as a Windows file name (§ is legal and preserved).
fn sanitize_windows_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    if s.is_empty() {
        s = "pack".into();
    }
    s
}

/// Extracts a zip with decoded entry names, guarding against path traversal.
fn extract_zip_to_dir(zip_path: &Path, dst: &Path) -> std::io::Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| std::io::Error::other(e.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let decoded = decode_entry_name(&entry.name_raw().to_vec()).replace('\\', "/");
        let mut safe = dst.to_path_buf();
        let mut valid = true;
        for comp in decoded.split('/') {
            if comp.is_empty() || comp == "." {
                continue;
            }
            if comp == ".." {
                valid = false;
                break;
            }
            safe.push(sanitize_windows_name(comp));
        }
        if !valid || safe == dst {
            continue;
        }
        if let Some(parent) = safe.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&safe)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

/// Root-level repair: move the original into problematic_packs, then rebuild
/// a clean normal zip in resourcepacks. Any failure leaves the original safe.
fn convert_pack(
    src: &Path,
    entry: &PackEntry,
    problematic_dir: &Path,
    resourcepacks: &Path,
) -> PackOutcome {
    let moved = match move_into(src, problematic_dir) {
        Ok(p) => p,
        Err(e) => {
            let action = if e.raw_os_error() == Some(LOCKED_OS_ERROR) {
                "skipped_locked"
            } else {
                "failed"
            };
            return PackOutcome {
                original_name: entry.name.clone(),
                action: action.into(),
                products: Vec::new(),
                causes: entry.causes.clone(),
                detail: Some(e.to_string()),
            };
        }
    };
    match build_root_fixed_product(&moved, &entry.name, resourcepacks) {
        Ok(product) => PackOutcome {
            original_name: entry.name.clone(),
            action: "converted".into(),
            products: vec![product],
            causes: entry.causes.clone(),
            detail: None,
        },
        Err(e) => PackOutcome {
            original_name: entry.name.clone(),
            action: "failed".into(),
            products: Vec::new(),
            causes: entry.causes.clone(),
            detail: Some(e.to_string()),
        },
    }
}

/// Per-entry decision list for rebuilding a pack's root layer.
struct FixPlan {
    map: std::collections::HashMap<String, String>,
    mcmeta_missing: bool,
}

fn build_root_fix_plan(names: &[String]) -> FixPlan {
    let has_mc = names.iter().any(|n| n == "pack.mcmeta");
    let has_png = names.iter().any(|n| n == "pack.png");
    let chosen_mc: Option<&String> = if has_mc {
        None
    } else {
        names
            .iter()
            .find(|n| !n.contains('/') && is_mcmeta_variant(n))
    };
    let chosen_png: Option<&String> = if has_png {
        None
    } else {
        names.iter().find(|n| !n.contains('/') && is_png_variant(n))
    };
    let mut map = std::collections::HashMap::new();
    for n in names {
        if n.ends_with('/') {
            continue;
        }
        if n.split('/').any(is_junk_name) || is_dead_path(n) {
            continue;
        }
        let target = if !n.contains('/') {
            if n == "pack.mcmeta" || n == "pack.png" {
                Some(n.clone())
            } else if Some(n) == chosen_mc {
                Some("pack.mcmeta".to_string())
            } else if Some(n) == chosen_png {
                Some("pack.png".to_string())
            } else {
                None
            }
        } else if n.starts_with("assets/") {
            Some(n.clone())
        } else {
            None
        };
        if let Some(t) = target {
            map.insert(n.clone(), t);
        }
    }
    FixPlan {
        mcmeta_missing: !has_mc && chosen_mc.is_none(),
        map,
    }
}

fn generated_mcmeta(stem: &str) -> String {
    let escaped = stem.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{{\"pack\":{{\"pack_format\":1,\"description\":\"{escaped}\"}}}}")
}

fn product_stem(original_name: &str, was_dir: bool) -> String {
    if was_dir {
        return original_name.to_string();
    }
    file_stem_of(original_name)
}

/// Rebuilds `moved` (a pack sitting in problematic_packs) as a clean zip in
/// `resourcepacks`, returning the product file name.
fn build_root_fixed_product(
    moved: &Path,
    original_name: &str,
    resourcepacks: &Path,
) -> std::io::Result<String> {
    let was_dir = moved.is_dir();
    let stem = product_stem(original_name, was_dir);
    let final_path = unique_target(resourcepacks, &format!("{stem}.zip"));
    let tmp_path = final_path.with_extension("zip.plot-partial");

    let result = write_fixed_zip(moved, was_dir, &stem, &tmp_path);
    match result {
        Ok(()) => {
            std::fs::rename(&tmp_path, &final_path)?;
            Ok(final_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn write_fixed_zip(
    moved: &Path,
    was_dir: bool,
    stem: &str,
    tmp_path: &Path,
) -> std::io::Result<()> {
    let out = std::fs::File::create(tmp_path)?;
    let mut writer = zip::ZipWriter::new(out);
    let options: zip::write::SimpleFileOptions = Default::default();

    let plan;
    if was_dir {
        let mut names = Vec::new();
        collect_relative_files(moved, "", &mut names)?;
        plan = build_root_fix_plan(&names);
        for name in &names {
            let Some(target) = plan.map.get(name) else {
                continue;
            };
            writer.start_file(target.as_str(), options)?;
            let src = moved.join(name.replace('/', "\\"));
            if target == "pack.mcmeta" {
                let bytes = std::fs::read(&src)?;
                std::io::Write::write_all(&mut writer, &product_mcmeta(bytes, stem))?;
            } else {
                let mut f = std::fs::File::open(&src)?;
                std::io::copy(&mut f, &mut writer)?;
            }
        }
    } else {
        let file = std::fs::File::open(moved)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let names: Vec<String> = archive.file_names().map(|n| n.to_string()).collect();
        plan = build_root_fix_plan(&names);
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let Some(target) = plan.map.get(&name) else {
                continue;
            };
            writer.start_file(target.as_str(), options)?;
            if target == "pack.mcmeta" {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                std::io::Write::write_all(&mut writer, &product_mcmeta(bytes, stem))?;
            } else {
                std::io::copy(&mut entry, &mut writer)?;
            }
        }
    }
    if plan.mcmeta_missing {
        writer.start_file("pack.mcmeta", options)?;
        std::io::Write::write_all(&mut writer, generated_mcmeta(stem).as_bytes())?;
    }
    writer
        .finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// Collects '/'-joined relative file paths under `dir`.
fn collect_relative_files(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<String>,
) -> std::io::Result<()> {
    for item in std::fs::read_dir(dir)?.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let p = item.path();
        if p.is_dir() {
            collect_relative_files(&p, &rel, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// Picks a non-colliding target name inside `dir` by appending " (n)".
fn unique_target(dir: &Path, name: &str) -> std::path::PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1.. {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

/// Moves a file or directory into `target_dir`, renaming on collision.
/// Falls back to copy+delete across volumes.
fn move_into(src: &Path, target_dir: &Path) -> std::io::Result<std::path::PathBuf> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = unique_target(target_dir, &name);
    if std::fs::rename(src, &target).is_ok() {
        return Ok(target);
    }
    if src.is_dir() {
        copy_dir(src, &target)?;
        std::fs::remove_dir_all(src)?;
    } else {
        std::fs::copy(src, &target)?;
        std::fs::remove_file(src)?;
    }
    Ok(target)
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for item in std::fs::read_dir(src)?.flatten() {
        let p = item.path();
        let t = dst.join(item.file_name());
        if p.is_dir() {
            copy_dir(&p, &t)?;
        } else {
            std::fs::copy(&p, &t)?;
        }
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for item in rd.flatten() {
            let p = item.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = std::fs::metadata(&p) {
                total += m.len();
            }
        }
    }
    total
}

fn classify_file(path: &Path, name: &str) -> (Category, Vec<String>) {
    let mut magic = [0u8; 6];
    let n = std::fs::File::open(path)
        .and_then(|mut f| {
            let mut read = 0;
            while read < magic.len() {
                match f.read(&mut magic[read..]) {
                    Ok(0) => break,
                    Ok(k) => read += k,
                    Err(e) => return Err(e),
                }
            }
            Ok(read)
        })
        .unwrap_or(0);
    if n < 2 || &magic[..2] != b"PK" {
        let cause = if n >= 4 && &magic[..4] == b"Rar!" {
            "rar_archive"
        } else if n >= 6 && magic == *b"7z\xbc\xaf\x27\x1c" {
            "sevenz_archive"
        } else {
            "not_zip"
        };
        return (Category::Illegal, vec![cause.into()]);
    }
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (Category::Illegal, vec!["corrupt_zip".into()]),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return (Category::Illegal, vec!["corrupt_zip".into()]),
    };
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index_raw(i) {
            if entry.encrypted() {
                return (Category::Illegal, vec!["encrypted_zip".into()]);
            }
        }
    }
    let names: Vec<String> = archive.file_names().map(|n| n.to_string()).collect();
    let facts = facts_from_zip_names(&names);
    if facts.has_assets && facts.has_mcmeta {
        if !name.ends_with(".zip") {
            return (Category::Nested, vec!["wrong_extension".into()]);
        }
        let causes = facts.bloat_causes();
        if !causes.is_empty() {
            return (Category::Bloated, causes);
        }
        return (Category::Normal, Vec::new());
    }
    if facts.has_assets {
        return (Category::Nested, vec!["mcmeta_rescue".into()]);
    }
    let mut search = CoreSearch::default();
    search_zip(&mut archive, &names, "", 1, &mut search);
    verdict_from_search(search)
}

/// A wrongly-cased or typo'd pack.mcmeta that a rename can rescue.
fn is_mcmeta_variant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower == "pack.mcmeta" && name != "pack.mcmeta")
        || lower == "pack.mcmeta.mcmeta"
        || lower == "pack.mcmeta.txt"
}

/// A wrongly-cased or typo'd pack.png that a rename can rescue.
fn is_png_variant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower == "pack.png" && name != "pack.png") || lower == "pack..png" || lower == "pack.png.png"
}

/// Paths under assets/ that no Minecraft version ever reads (the Yokabi disease).
fn is_dead_path(entry_name: &str) -> bool {
    let parts: Vec<&str> = entry_name.split('/').collect();
    parts.len() >= 3 && parts[0] == "assets" && parts[2] == "records"
}
