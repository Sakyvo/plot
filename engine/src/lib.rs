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
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Pack,
    ClassificationFolder,
    SupportingFolder,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoreReason {
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackEntry {
    pub name: String,
    pub relative_path: String,
    pub parent_path: Option<String>,
    pub kind: NodeKind,
    pub category: Category,
    /// Machine-readable cause keys; the UI maps them to localized text.
    pub causes: Vec<String>,
    pub size_bytes: u64,
    pub ignore: Option<IgnoreReason>,
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
    pub ignored: usize,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub path: String,
    pub status: ScanStatus,
    pub total_packs: usize,
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
            total_packs: 0,
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
    let entry_groups: Vec<Vec<PackEntry>> = items
        .into_par_iter()
        .map(|(name, item_path, is_dir)| {
            let mut group = if is_dir {
                scan_fs_directory_node(name.clone(), item_path.clone(), name.clone(), None)
            } else {
                scan_file_node(name.clone(), item_path.clone(), name.clone(), None)
            };
            let index = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            on_progress(&ScanProgress {
                name: name.clone(),
                index,
                total,
            });
            std::mem::take(&mut group)
        })
        .collect();
    let entries: Vec<PackEntry> = entry_groups.into_iter().flatten().collect();
    let mut counts = Counts::default();
    for e in &entries {
        match e.kind {
            NodeKind::ClassificationFolder => counts.folder += 1,
            NodeKind::SupportingFolder => continue,
            NodeKind::Shell => counts.nested += 1,
            NodeKind::Pack => match e.category {
                Category::Normal => counts.normal += 1,
                Category::Nested => counts.nested += 1,
                Category::Folder => {}
                Category::Bloated => counts.bloated += 1,
                Category::Illegal => counts.illegal += 1,
                Category::LunarIllegal => {}
                Category::Ignored => counts.ignored += 1,
            },
        }
        if e.category == Category::LunarIllegal || e.causes.iter().any(|c| c == "lunar_escape") {
            counts.lunar += 1;
        }
    }
    let total_packs = entries
        .iter()
        .filter(|entry| entry.kind == NodeKind::Pack)
        .count();
    let status = if total_packs == 0 {
        ScanStatus::NoPacks
    } else {
        ScanStatus::Ok
    };
    ScanReport {
        path,
        status,
        total_packs,
        entries,
        counts,
    }
}

fn scan_fs_directory_node(
    name: String,
    path: std::path::PathBuf,
    relative_path: String,
    parent_path: Option<String>,
) -> Vec<PackEntry> {
    if root_has_assets(&path, true) {
        return vec![classify_pack_entry(
            name,
            path,
            true,
            relative_path,
            parent_path,
        )];
    }
    let children = direct_pack_children(&path);
    if children.len() == 1 {
        let has_attachment = has_non_junk_attachment(&path);
        let mut entries = vec![PackEntry {
            name,
            relative_path: relative_path.clone(),
            parent_path,
            kind: if has_attachment {
                NodeKind::SupportingFolder
            } else {
                NodeKind::Shell
            },
            category: if has_attachment {
                Category::Normal
            } else {
                Category::Nested
            },
            causes: if has_attachment {
                Vec::new()
            } else {
                vec!["folder_shell".into()]
            },
            size_bytes: dir_size(&path),
            ignore: None,
        }];
        let (child_name, child_path, child_is_dir) = children.into_iter().next().unwrap();
        let child_relative_path = format!("{relative_path}/{child_name}");
        if child_is_dir {
            entries.extend(scan_fs_directory_node(
                child_name,
                child_path,
                child_relative_path,
                Some(relative_path),
            ));
        } else {
            entries.extend(scan_file_node(
                child_name,
                child_path,
                child_relative_path,
                Some(relative_path),
            ));
        }
        return entries;
    }
    if children.len() < 2 {
        return vec![classify_pack_entry(
            name,
            path,
            true,
            relative_path,
            parent_path,
        )];
    }
    let mut entries = vec![PackEntry {
        name,
        relative_path: relative_path.clone(),
        parent_path,
        kind: NodeKind::ClassificationFolder,
        category: Category::Folder,
        causes: Vec::new(),
        size_bytes: dir_size(&path),
        ignore: None,
    }];
    for (child_name, child_path, child_is_dir) in children {
        let child_relative_path = format!("{relative_path}/{child_name}");
        if child_is_dir {
            entries.extend(scan_fs_directory_node(
                child_name,
                child_path,
                child_relative_path,
                Some(relative_path.clone()),
            ));
        } else {
            entries.extend(scan_file_node(
                child_name,
                child_path,
                child_relative_path,
                Some(relative_path.clone()),
            ));
        }
    }
    entries
}

fn scan_file_node(
    name: String,
    path: std::path::PathBuf,
    relative_path: String,
    parent_path: Option<String>,
) -> Vec<PackEntry> {
    archive_shell_entries(&name, &path, &relative_path, parent_path.clone()).unwrap_or_else(|| {
        vec![classify_pack_entry(
            name,
            path,
            false,
            relative_path,
            parent_path,
        )]
    })
}

fn archive_shell_entries(
    name: &str,
    path: &Path,
    relative_path: &str,
    parent_path: Option<String>,
) -> Option<Vec<PackEntry>> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let (names, _) = decoded_zip_names(&mut archive);
    if zip_layer_has_assets(&names, "") {
        return None;
    }
    let mut children = Vec::new();
    let mut has_root_attachment = false;
    for child_name in zip_child_files(&names, "") {
        let bytes = read_decoded_zip_file(&mut archive, &child_name)?;
        if !is_container_payload(&child_name, &bytes) {
            has_root_attachment = true;
            continue;
        }
        children.push(classify_archive_child(
            child_name.clone(),
            bytes,
            format!("{relative_path}/{child_name}"),
            Some(relative_path.to_string()),
        ));
    }
    for child_name in zip_child_dirs(&names, "") {
        let prefix = format!("{child_name}/");
        let entries = scan_archive_directory_node(
            child_name.clone(),
            &mut archive,
            &names,
            &prefix,
            format!("{relative_path}/{child_name}"),
            Some(relative_path.to_string()),
            1,
        );
        if entries.is_empty() {
            has_root_attachment = true;
        } else {
            children.extend(entries);
        }
    }
    if children.is_empty() {
        return None;
    }
    let mut causes = vec!["archive_shell".into()];
    if has_root_attachment {
        causes.push("archive_root_attachments".into());
    }
    let mut entries = vec![PackEntry {
        name: name.to_string(),
        relative_path: relative_path.to_string(),
        parent_path,
        kind: NodeKind::Shell,
        category: Category::Nested,
        causes,
        size_bytes: std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        ignore: None,
    }];
    entries.append(&mut children);
    Some(entries)
}

fn scan_archive_directory_node<R: Read + Seek>(
    name: String,
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
    relative_path: String,
    parent_path: Option<String>,
    depth: usize,
) -> Vec<PackEntry> {
    if depth > MAX_NESTING_DEPTH {
        return Vec::new();
    }
    if zip_layer_has_assets(names, prefix) {
        return vec![classify_archive_folder_child(
            name,
            archive,
            names,
            prefix,
            relative_path,
            parent_path,
        )];
    }

    let mut child_groups = Vec::new();
    for child_name in zip_child_files(names, prefix) {
        let full_name = format!("{prefix}{child_name}");
        let Some(bytes) = read_decoded_zip_file(archive, &full_name) else {
            continue;
        };
        if !is_container_payload(&child_name, &bytes) {
            continue;
        }
        child_groups.push(vec![classify_archive_child(
            child_name.clone(),
            bytes,
            format!("{relative_path}/{child_name}"),
            Some(relative_path.clone()),
        )]);
    }
    for child_name in zip_child_dirs(names, prefix) {
        let child_prefix = format!("{prefix}{child_name}/");
        let entries = scan_archive_directory_node(
            child_name.clone(),
            archive,
            names,
            &child_prefix,
            format!("{relative_path}/{child_name}"),
            Some(relative_path.clone()),
            depth + 1,
        );
        if !entries.is_empty() {
            child_groups.push(entries);
        }
    }
    if child_groups.is_empty() {
        return Vec::new();
    }

    let has_attachment = archive_directory_has_attachment(archive, names, prefix, &child_groups);
    let (kind, category, causes) = if child_groups.len() >= 2 {
        (NodeKind::ClassificationFolder, Category::Folder, Vec::new())
    } else if has_attachment {
        (NodeKind::SupportingFolder, Category::Normal, Vec::new())
    } else {
        (
            NodeKind::Shell,
            Category::Nested,
            vec!["folder_shell".into()],
        )
    };
    let mut entries = vec![PackEntry {
        name,
        relative_path: relative_path.clone(),
        parent_path,
        kind,
        category,
        causes,
        size_bytes: archive_prefix_size(archive, names, prefix),
        ignore: None,
    }];
    entries.extend(child_groups.into_iter().flatten());
    entries
}

fn archive_directory_has_attachment<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
    child_groups: &[Vec<PackEntry>],
) -> bool {
    for child_name in zip_child_files(names, prefix) {
        let full_name = format!("{prefix}{child_name}");
        let is_pack = read_decoded_zip_file(archive, &full_name)
            .map(|bytes| is_container_payload(&child_name, &bytes))
            .unwrap_or(false);
        if !is_pack {
            return true;
        }
    }
    let valid_dirs: BTreeSet<&str> = child_groups
        .iter()
        .filter_map(|entries| entries.first().map(|entry| entry.name.as_str()))
        .collect();
    zip_child_dirs(names, prefix)
        .iter()
        .any(|name| !valid_dirs.contains(name.as_str()))
}

fn archive_prefix_size<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
) -> u64 {
    names
        .iter()
        .filter(|name| name.starts_with(prefix) && !name.ends_with('/'))
        .filter_map(|name| archive.by_name(name).ok().map(|entry| entry.size()))
        .sum()
}

fn classify_archive_folder_child<R: Read + Seek>(
    name: String,
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
    relative_path: String,
    parent_path: Option<String>,
) -> PackEntry {
    let stripped_names: Vec<String> = names
        .iter()
        .filter_map(|entry| entry.strip_prefix(prefix).map(str::to_string))
        .collect();
    let stripped_files: Vec<String> = stripped_names
        .iter()
        .filter(|entry| !entry.ends_with('/'))
        .cloned()
        .collect();
    let facts = facts_from_zip_names(&stripped_names);
    let mut category;
    let mut causes;
    if facts.has_assets && facts.has_mcmeta {
        category = Category::Bloated;
        causes = vec!["folder_pack".into()];
        causes.extend(facts.bloat_causes());
    } else if facts.has_assets {
        category = Category::Nested;
        causes = vec!["mcmeta_rescue".into()];
    } else {
        category = Category::Illegal;
        causes = vec!["no_core_found".into()];
    }
    let modern = core_modern_texture_paths(&stripped_names, &stripped_files, "");
    let ignore = (!modern.is_empty()).then(|| IgnoreReason {
        key: "modern_texture_layout".into(),
        values: modern,
    });
    if ignore.is_some() {
        category = Category::Ignored;
        causes.clear();
    } else if category != Category::Illegal {
        if let Some(raw) = read_decoded_zip_file(archive, &format!("{prefix}pack.mcmeta")) {
            if mcmeta_has_lunar_escape(&raw) {
                causes.push("lunar_escape".into());
            }
        }
    }
    PackEntry {
        name,
        relative_path,
        parent_path,
        kind: NodeKind::Pack,
        category,
        causes,
        size_bytes: 0,
        ignore,
    }
}

fn classify_archive_child(
    name: String,
    bytes: Vec<u8>,
    relative_path: String,
    parent_path: Option<String>,
) -> PackEntry {
    let (mut category, mut causes) = classify_file_bytes(&bytes, &name);
    let ignore = if category == Category::Illegal {
        None
    } else {
        high_version_ignore_reason_bytes(&bytes, category)
    };
    if ignore.is_some() {
        category = Category::Ignored;
        causes.clear();
    } else if category != Category::Illegal {
        if let Some(raw) = read_judgment_mcmeta_bytes(&bytes) {
            if mcmeta_has_lunar_escape(&raw) {
                causes.push("lunar_escape".into());
                if category == Category::Normal {
                    category = Category::LunarIllegal;
                }
            }
        }
    }
    PackEntry {
        name,
        relative_path,
        parent_path,
        kind: NodeKind::Pack,
        category,
        causes,
        size_bytes: bytes.len() as u64,
        ignore,
    }
}

fn high_version_ignore_reason_bytes(bytes: &[u8], category: Category) -> Option<IgnoreReason> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let (names, files) = decoded_zip_names(&mut archive);
    let mut modern = BTreeSet::new();
    if zip_layer_has_assets(&names, "") {
        modern.extend(core_modern_texture_paths(&names, &files, ""));
    } else if category == Category::Nested {
        collect_zip_modern_texture_paths(&mut archive, &names, &files, "", 1, &mut modern);
    }
    (!modern.is_empty()).then(|| IgnoreReason {
        key: "modern_texture_layout".into(),
        values: modern.into_iter().collect(),
    })
}

fn read_judgment_mcmeta_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let (names, _) = decoded_zip_names(&mut archive);
    if let Some(bytes) = read_decoded_zip_file(&mut archive, "pack.mcmeta") {
        return Some(bytes);
    }
    for name in names {
        if !name.ends_with("pack.mcmeta") {
            continue;
        }
        let prefix = name.trim_end_matches("pack.mcmeta");
        if zip_layer_has_assets(
            &archive.file_names().map(str::to_string).collect::<Vec<_>>(),
            prefix,
        ) {
            return read_decoded_zip_file(&mut archive, &name);
        }
    }
    None
}

fn classify_pack_entry(
    name: String,
    path: std::path::PathBuf,
    is_dir: bool,
    relative_path: String,
    parent_path: Option<String>,
) -> PackEntry {
    let (mut category, mut causes) = if is_dir {
        classify_dir(&path)
    } else {
        classify_file(&path, &name)
    };
    let ignore = if category == Category::Illegal {
        None
    } else {
        high_version_ignore_reason(&path, is_dir, category)
    };
    if ignore.is_some() {
        category = Category::Ignored;
        causes.clear();
    } else {
        (category, causes) = apply_lunar_tag(category, causes, &path, is_dir);
    }
    let size_bytes = if is_dir {
        dir_size(&path)
    } else {
        std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
    };
    PackEntry {
        name,
        relative_path,
        parent_path,
        kind: NodeKind::Pack,
        category,
        causes,
        size_bytes,
        ignore,
    }
}

fn direct_pack_children(dir: &Path) -> Vec<(String, std::path::PathBuf, bool)> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read_dir
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            if is_junk_name(&name) {
                return None;
            }
            let path = item.path();
            let is_dir = path.is_dir();
            let intended_pack = is_pack_intended_child(&name, &path, is_dir);
            intended_pack.then_some((name, path, is_dir))
        })
        .collect()
}

fn is_pack_intended_child(name: &str, path: &Path, is_dir: bool) -> bool {
    if is_dir {
        return folder_contains_core(path, 1);
    }
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".rar")
        || lower.ends_with(".7z")
        || std::fs::read(path)
            .map(|bytes| is_container_payload(name, &bytes))
            .unwrap_or(false)
}

fn has_non_junk_attachment(dir: &Path) -> bool {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return false;
    };
    read_dir.flatten().any(|item| {
        let name = item.file_name().to_string_lossy().into_owned();
        if is_junk_name(&name) {
            return false;
        }
        let path = item.path();
        !is_pack_intended_child(&name, &path, path.is_dir())
    })
}

fn mcmeta_candidate_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    mcmeta_candidate_rank(a)
        .cmp(&mcmeta_candidate_rank(b))
        .then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()))
        .then_with(|| a.cmp(b))
}

fn mcmeta_candidate_rank(name: &str) -> u8 {
    if name == "pack.mcmeta" {
        0
    } else if name.eq_ignore_ascii_case("pack.mcmeta") {
        1
    } else if name.eq_ignore_ascii_case("pack.mcmeta.mcmeta") {
        2
    } else {
        3
    }
}

fn root_has_assets(path: &Path, is_dir: bool) -> bool {
    if is_dir {
        return path.join("assets").is_dir();
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let found = archive.file_names().any(|name| name.starts_with("assets/"));
    found
}

const MODERN_TEXTURE_DIRS: [&str; 2] = [
    "assets/minecraft/textures/item",
    "assets/minecraft/textures/block",
];
const LEGACY_TEXTURE_DIRS: [&str; 2] = [
    "assets/minecraft/textures/items",
    "assets/minecraft/textures/blocks",
];

fn root_modern_texture_reason(path: &Path, is_dir: bool) -> Option<IgnoreReason> {
    let modern = if is_dir {
        fs_core_modern_texture_paths(path)
    } else {
        let file = std::fs::File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let (names, files) = decoded_zip_names(&mut archive);
        core_modern_texture_paths(&names, &files, "")
    };
    (!modern.is_empty()).then(|| IgnoreReason {
        key: "modern_texture_layout".into(),
        values: modern,
    })
}

fn high_version_ignore_reason(
    path: &Path,
    is_dir: bool,
    category: Category,
) -> Option<IgnoreReason> {
    if root_has_assets(path, is_dir) {
        return root_modern_texture_reason(path, is_dir);
    }
    (category == Category::Nested)
        .then(|| nested_modern_texture_reason(path, is_dir))
        .flatten()
}

fn names_contain_files(names: &[String], directory: &str) -> bool {
    let prefix = format!("{directory}/");
    names.iter().any(|name| name.starts_with(&prefix))
}

fn names_contain_directory(names: &[String], directory: &str) -> bool {
    let prefix = format!("{directory}/");
    names
        .iter()
        .any(|name| name == &prefix || name.starts_with(&prefix))
}

fn fs_core_modern_texture_paths(path: &Path) -> Vec<String> {
    if LEGACY_TEXTURE_DIRS
        .iter()
        .any(|relative| path.join(relative).is_dir())
    {
        return Vec::new();
    }
    let mut modern = Vec::new();
    if dir_contains_file(&path.join(MODERN_TEXTURE_DIRS[0])) {
        modern.push(MODERN_TEXTURE_DIRS[0].to_string());
    }
    if path.join(MODERN_TEXTURE_DIRS[1]).is_dir() {
        modern.push(MODERN_TEXTURE_DIRS[1].to_string());
    }
    modern
}

fn dir_contains_file(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file() || (path.is_dir() && dir_contains_file(&path))
    })
}

fn nested_modern_texture_reason(path: &Path, is_dir: bool) -> Option<IgnoreReason> {
    let mut modern = BTreeSet::new();
    if is_dir {
        collect_fs_modern_texture_paths(path, 1, &mut modern);
    } else {
        let file = std::fs::File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let (names, files) = decoded_zip_names(&mut archive);
        collect_zip_modern_texture_paths(&mut archive, &names, &files, "", 1, &mut modern);
    }
    (!modern.is_empty()).then(|| IgnoreReason {
        key: "modern_texture_layout".into(),
        values: modern.into_iter().collect(),
    })
}

fn collect_fs_modern_texture_paths(dir: &Path, child_depth: usize, out: &mut BTreeSet<String>) {
    if child_depth > MAX_NESTING_DEPTH {
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
                if let Some(reason) = root_modern_texture_reason(&p, true) {
                    out.extend(reason.values);
                }
            } else {
                collect_fs_modern_texture_paths(&p, child_depth + 1, out);
            }
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if !bytes.starts_with(b"PK") {
            continue;
        }
        let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
            continue;
        };
        let (names, files) = decoded_zip_names(&mut archive);
        if zip_layer_has_assets(&names, "") {
            out.extend(core_modern_texture_paths(&names, &files, ""));
        } else {
            collect_zip_modern_texture_paths(
                &mut archive,
                &names,
                &files,
                "",
                child_depth + 1,
                out,
            );
        }
    }
}

fn collect_zip_modern_texture_paths<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    files: &[String],
    prefix: &str,
    child_depth: usize,
    out: &mut BTreeSet<String>,
) {
    if child_depth > MAX_NESTING_DEPTH {
        return;
    }
    for dir in zip_child_dirs(names, prefix) {
        let layer = format!("{prefix}{dir}/");
        if zip_layer_has_assets(names, &layer) {
            out.extend(core_modern_texture_paths(names, files, &layer));
        } else {
            collect_zip_modern_texture_paths(archive, names, files, &layer, child_depth + 1, out);
        }
    }
    for file_name in zip_child_files(names, prefix) {
        let full = format!("{prefix}{file_name}");
        let Some(bytes) = read_decoded_zip_file(archive, &full) else {
            continue;
        };
        if !bytes.starts_with(b"PK") {
            continue;
        }
        let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(bytes)) else {
            continue;
        };
        let (inner_names, inner_files) = decoded_zip_names(&mut inner);
        if zip_layer_has_assets(&inner_names, "") {
            out.extend(core_modern_texture_paths(&inner_names, &inner_files, ""));
        } else {
            collect_zip_modern_texture_paths(
                &mut inner,
                &inner_names,
                &inner_files,
                "",
                child_depth + 1,
                out,
            );
        }
    }
}

fn decoded_zip_names<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(index) else {
            continue;
        };
        let name = decode_entry_name(entry.name_raw());
        if entry.is_file() {
            files.push(name.clone());
        }
        names.push(name);
    }
    (names, files)
}

fn read_decoded_zip_file<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    target: &str,
) -> Option<Vec<u8>> {
    for index in 0..archive.len() {
        let matches = {
            let entry = archive.by_index_raw(index).ok()?;
            entry.is_file() && decode_entry_name(entry.name_raw()) == target
        };
        if !matches {
            continue;
        }
        let mut entry = archive.by_index(index).ok()?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

fn zip_layer_has_assets(names: &[String], prefix: &str) -> bool {
    let assets = format!("{prefix}assets/");
    names.iter().any(|name| name.starts_with(&assets))
}

fn core_modern_texture_paths(names: &[String], files: &[String], prefix: &str) -> Vec<String> {
    if LEGACY_TEXTURE_DIRS
        .iter()
        .any(|directory| names_contain_directory(names, &format!("{prefix}{directory}")))
    {
        return Vec::new();
    }
    let mut modern = Vec::new();
    if names_contain_files(files, &format!("{prefix}{}", MODERN_TEXTURE_DIRS[0])) {
        modern.push(MODERN_TEXTURE_DIRS[0].to_string());
    }
    if names_contain_directory(names, &format!("{prefix}{}", MODERN_TEXTURE_DIRS[1])) {
        modern.push(MODERN_TEXTURE_DIRS[1].to_string());
    }
    modern
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
        let mut causes = vec!["folder_pack".into()];
        causes.extend(facts.bloat_causes());
        return (Category::Bloated, causes);
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
                    let inames: Vec<String> = inner.file_names().map(|n| n.to_string()).collect();
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
        let has_assets = names
            .iter()
            .any(|n| n.starts_with(&format!("{layer}assets/")));
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
    /// Name of this batch's run folder inside `plot_temp` — injected by the
    /// caller (see [`default_run_dir_name`]) so the engine never reads a clock.
    pub run_dir_name: String,
}

/// `Plot_YYYY-MM-DD_HH.mm.ss` — local time with dots, colons are illegal in
/// Windows file names. Callers pass the result as [`ProcessOptions::run_dir_name`].
pub fn default_run_dir_name() -> String {
    let now = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let format = time::macros::format_description!("Plot_[year]-[month]-[day]_[hour].[minute].[second]");
    now.format(&format)
        .unwrap_or_else(|_| format!("Plot_{}", now.unix_timestamp()))
}

struct ProcessTargets<'a> {
    problematic: &'a Path,
    illegal: &'a Path,
    resource: &'a Path,
    plot_temp: &'a Path,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackOutcome {
    pub original_name: String,
    /// Machine key: moved_to_illegal | skipped_unsupported | converted | failed | skipped_locked
    pub action: String,
    pub products: Vec<String>,
    pub causes: Vec<String>,
    pub detail: Option<String>,
    pub separated: Vec<SeparatedPack>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeparatedPack {
    pub name: String,
    pub parent: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessReport {
    pub outcomes: Vec<PackOutcome>,
    pub notices: Vec<ProcessNotice>,
    /// The run folder actually used (plain name inside plot_temp, with any
    /// collision suffix). `None` when the batch had nothing to do.
    pub run_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessNotice {
    pub key: String,
    pub values: Vec<String>,
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
    let report = scan(&opts.resourcepacks);
    let by_path: std::collections::HashMap<&str, &PackEntry> = report
        .entries
        .iter()
        .map(|entry| (entry.relative_path.as_str(), entry))
        .collect();
    let has_shell_ancestor = |entry: &PackEntry| {
        let mut parent = entry.parent_path.as_deref();
        while let Some(path) = parent {
            let Some(ancestor) = by_path.get(path) else {
                break;
            };
            if ancestor.kind == NodeKind::Shell {
                return true;
            }
            parent = ancestor.parent_path.as_deref();
        }
        false
    };
    let work: Vec<&PackEntry> = report
        .entries
        .iter()
        .filter(|e| {
            (e.kind == NodeKind::Shell && !has_shell_ancestor(e))
                || (e.kind == NodeKind::Pack
                    && !has_shell_ancestor(e)
                    && e.category != Category::Ignored
                    && (e.category != Category::Normal
                        || e.causes.iter().any(|c| c == "lunar_escape")))
        })
        .collect();
    if work.is_empty() {
        return Ok(ProcessReport {
            outcomes: Vec::new(),
            notices: Vec::new(),
            run_dir: None,
        });
    }
    let run_root = unique_run_root(&opts.plot_temp, &opts.run_dir_name);
    let illegal_dir = run_root.join("illegal_packs");
    let problematic_dir = run_root.join("problematic_packs");
    let run_dir = run_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    std::fs::create_dir_all(&illegal_dir)
        .map_err(|e| ProcessError::PlotTempNotWritable(e.to_string()))?;
    std::fs::create_dir_all(&problematic_dir)
        .map_err(|e| ProcessError::PlotTempNotWritable(e.to_string()))?;
    let total = work.len();
    let mut outcomes = Vec::new();
    let mut notices = Vec::new();
    for (index, entry) in work.into_iter().enumerate() {
        on_progress(&ProgressEvent {
            name: entry.relative_path.clone(),
            index,
            total,
        });
        let src = opts.resourcepacks.join(&entry.relative_path);
        let relative_parent = Path::new(&entry.relative_path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(""));
        let resource_target = opts.resourcepacks.join(relative_parent);
        let problematic_target = problematic_dir.join(relative_parent);
        let illegal_target = illegal_dir.join(relative_parent);
        let target_result = std::fs::create_dir_all(&problematic_target)
            .and_then(|_| std::fs::create_dir_all(&illegal_target));
        let archive_shell = entry.kind == NodeKind::Shell
            && entry.causes.iter().any(|cause| cause == "archive_shell");
        let mut entry_outcomes = if let Err(error) = target_result {
            vec![locked_or_failed_outcome(entry, &error)]
        } else {
            if archive_shell {
                if entry
                    .causes
                    .iter()
                    .any(|cause| cause == "archive_root_attachments")
                {
                    notices.push(ProcessNotice {
                        key: "attachments_kept_in_original_archive".into(),
                        values: vec![entry.relative_path.clone()],
                    });
                }
                let children: Vec<&PackEntry> = report
                    .entries
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .relative_path
                            .strip_prefix(&entry.relative_path)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                    })
                    .collect();
                process_archive_shell(
                    &src,
                    entry,
                    &children,
                    ProcessTargets {
                        problematic: &problematic_target,
                        illegal: &illegal_target,
                        resource: &resource_target,
                        plot_temp: &run_root,
                    },
                    relative_parent,
                )
            } else if entry.kind == NodeKind::Shell {
                let child = report.entries.iter().find(|candidate| {
                    candidate.parent_path.as_deref() == Some(&entry.relative_path)
                });
                match child {
                    Some(child)
                        if child.kind == NodeKind::Shell
                            && child.causes.iter().any(|cause| cause == "archive_shell") =>
                    {
                        if child
                            .causes
                            .iter()
                            .any(|cause| cause == "archive_root_attachments")
                        {
                            notices.push(ProcessNotice {
                                key: "attachments_kept_in_original_archive".into(),
                                values: vec![child.relative_path.clone()],
                            });
                        }
                        let descendants: Vec<&PackEntry> = report
                            .entries
                            .iter()
                            .filter(|candidate| {
                                candidate
                                    .relative_path
                                    .strip_prefix(&child.relative_path)
                                    .is_some_and(|suffix| suffix.starts_with('/'))
                            })
                            .collect();
                        let backup_target = problematic_target.join(&entry.name);
                        let outcomes = match std::fs::create_dir_all(&backup_target) {
                            Ok(()) => process_archive_shell(
                                &src.join(&child.name),
                                child,
                                &descendants,
                                ProcessTargets {
                                    problematic: &backup_target,
                                    illegal: &illegal_target,
                                    resource: &resource_target,
                                    plot_temp: &run_root,
                                },
                                relative_parent,
                            ),
                            Err(error) => vec![locked_or_failed_outcome(child, &error)],
                        };
                        if outcomes.iter().all(|outcome| {
                            outcome.action != "failed" && outcome.action != "skipped_locked"
                        }) {
                            let _ = remove_shell_junk_and_dir(&src);
                        }
                        outcomes
                    }
                    Some(child) => vec![process_folder_shell(
                        &src,
                        entry,
                        child,
                        &problematic_target,
                        &illegal_target,
                        &resource_target,
                        &run_root,
                    )],
                    None => vec![PackOutcome {
                        original_name: entry.name.clone(),
                        action: "failed".into(),
                        products: Vec::new(),
                        causes: entry.causes.clone(),
                        detail: Some("folder shell has no child".into()),
                        separated: Vec::new(),
                    }],
                }
            } else {
                vec![match entry.category {
                    Category::Illegal => match move_into(&src, &illegal_target) {
                        Ok(_) => PackOutcome {
                            original_name: entry.name.clone(),
                            action: "moved_to_illegal".into(),
                            products: Vec::new(),
                            causes: entry.causes.clone(),
                            detail: None,
                            separated: Vec::new(),
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
                                separated: Vec::new(),
                            }
                        }
                    },
                    Category::Nested if entry.causes.iter().any(|c| c == "nested_container") => {
                        convert_nested_pack(
                            &src,
                            entry,
                            &problematic_target,
                            &resource_target,
                            &run_root,
                        )
                    }
                    _ => convert_pack(
                        &src,
                        entry,
                        &problematic_target,
                        &resource_target,
                        &run_root,
                    ),
                }]
            }
        };
        if !archive_shell {
            for outcome in &mut entry_outcomes {
                qualify_outcome_paths(entry, outcome);
            }
        }
        outcomes.extend(entry_outcomes);
    }
    Ok(ProcessReport {
        outcomes,
        notices,
        run_dir,
    })
}

fn process_archive_shell(
    shell_src: &Path,
    shell: &PackEntry,
    children: &[&PackEntry],
    targets: ProcessTargets<'_>,
    result_parent: &Path,
) -> Vec<PackOutcome> {
    let failed = |detail: String| {
        vec![PackOutcome {
            original_name: shell.relative_path.clone(),
            action: "failed".into(),
            products: Vec::new(),
            causes: shell.causes.clone(),
            detail: Some(detail),
            separated: Vec::new(),
        }]
    };
    let work_root = unique_target(
        &targets.plot_temp.join(".work"),
        &format!("{}.archive-shell", sanitize_windows_name(&shell.name)),
    );
    if let Err(error) = std::fs::create_dir_all(&work_root) {
        return failed(error.to_string());
    }
    let file = match std::fs::File::open(shell_src) {
        Ok(file) => file,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
    };
    let (archive_names, _) = decoded_zip_names(&mut archive);

    struct Commit {
        staged: std::path::PathBuf,
        target: std::path::PathBuf,
    }
    let mut commits = Vec::new();
    let mut outcomes = Vec::new();
    let by_path: std::collections::HashMap<&str, &PackEntry> = children
        .iter()
        .map(|entry| (entry.relative_path.as_str(), *entry))
        .collect();
    let pack_children: Vec<&PackEntry> = children
        .iter()
        .copied()
        .filter(|entry| entry.kind == NodeKind::Pack)
        .collect();
    let pack_sources: Vec<(String, bool)> = pack_children
        .iter()
        .map(|child| {
            let inner_path = child
                .relative_path
                .strip_prefix(&format!("{}/", shell.relative_path))
                .unwrap_or(&child.name)
                .to_string();
            let is_folder = !archive_names
                .iter()
                .any(|name| name == &inner_path && !name.ends_with('/'));
            (inner_path, is_folder)
        })
        .collect();
    let mut resource_reserved = Vec::new();
    let mut illegal_reserved = Vec::new();
    for (index, child) in pack_children.iter().enumerate() {
        let source = work_root.join(format!("source-{index}"));
        let inner_path = child
            .relative_path
            .strip_prefix(&format!("{}/", shell.relative_path))
            .unwrap_or(&child.name);
        let preserved_parent = archive_preserved_parent(shell, child, &by_path);
        let direct_bytes = read_decoded_zip_file(&mut archive, inner_path);
        let is_folder = direct_bytes.is_none();
        let prepared_source = if let Some(bytes) = &direct_bytes {
            std::fs::write(&source, bytes)
        } else {
            let prefix = format!("{inner_path}/");
            match read_zip_folder_entries(&mut archive, &prefix) {
                Ok(files) if !files.is_empty() => write_staged_folder(&source, files),
                Ok(_) => Err(std::io::Error::other(format!(
                    "cannot read inner pack {}",
                    child.relative_path
                ))),
                Err(error) => Err(error),
            }
        };
        if let Err(error) = prepared_source {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
        let mut products = Vec::new();
        let action;
        if child.category == Category::Illegal {
            action = "moved_to_illegal";
            let safe_name = sanitize_windows_name(&child.name);
            let target_dir = targets.illegal.join(&preserved_parent);
            let target = unique_target_reserved(&target_dir, &safe_name, &illegal_reserved);
            illegal_reserved.push(target.clone());
            let staged = work_root.join(format!("illegal-{index}"));
            let result = if is_folder {
                copy_dir(&source, &staged)
            } else {
                std::fs::copy(&source, &staged).map(|_| ())
            };
            if let Err(error) = result {
                let _ = std::fs::remove_dir_all(&work_root);
                return failed(error.to_string());
            }
            products.push(archive_shell_target_name(
                result_parent,
                targets.illegal,
                &target,
            ));
            commits.push(Commit { staged, target });
        } else if (child.category == Category::Normal || child.category == Category::Ignored)
            && !child.causes.iter().any(|cause| cause == "lunar_escape")
        {
            action = "converted";
            let safe_name = sanitize_windows_name(&child.name);
            let target_dir = targets.resource.join(&preserved_parent);
            let target = unique_target_reserved_ignoring(
                &target_dir,
                &safe_name,
                &resource_reserved,
                shell_src,
            );
            resource_reserved.push(target.clone());
            let staged = work_root.join(format!("unchanged-{index}"));
            let result = if is_folder {
                copy_dir(&source, &staged)
            } else {
                std::fs::copy(&source, &staged).map(|_| ())
            };
            if let Err(error) = result {
                let _ = std::fs::remove_dir_all(&work_root);
                return failed(error.to_string());
            }
            products.push(archive_shell_target_name(
                result_parent,
                targets.resource,
                &target,
            ));
            commits.push(Commit { staged, target });
        } else if child.category == Category::Nested
            && child.causes.iter().any(|cause| cause == "nested_container")
        {
            action = "converted";
            let staged_dir = work_root.join(format!("nested-products-{index}"));
            let extract = work_root.join(format!("nested-extract-{index}"));
            if let Err(error) = std::fs::create_dir_all(&staged_dir) {
                let _ = std::fs::remove_dir_all(&work_root);
                return failed(error.to_string());
            }
            let names = match unwrap_layers(&source, &child.name, &extract, &staged_dir) {
                Ok(names) => names,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&work_root);
                    return failed(error.to_string());
                }
            };
            for name in names {
                let target_dir = targets.resource.join(&preserved_parent);
                let target = unique_target_reserved_ignoring(
                    &target_dir,
                    &name,
                    &resource_reserved,
                    shell_src,
                );
                resource_reserved.push(target.clone());
                products.push(archive_shell_target_name(
                    result_parent,
                    targets.resource,
                    &target,
                ));
                commits.push(Commit {
                    staged: staged_dir.join(&name),
                    target,
                });
            }
        } else {
            action = "converted";
            let stem = sanitize_windows_name(&product_stem(&child.name, is_folder));
            let name = format!("{stem}.zip");
            let target_dir = targets.resource.join(&preserved_parent);
            let target =
                unique_target_reserved_ignoring(&target_dir, &name, &resource_reserved, shell_src);
            resource_reserved.push(target.clone());
            let staged = work_root.join(format!("fixed-{index}.zip"));
            if let Err(error) = write_fixed_zip(&source, is_folder, &stem, &staged) {
                let _ = std::fs::remove_dir_all(&work_root);
                return failed(error.to_string());
            }
            products.push(archive_shell_target_name(
                result_parent,
                targets.resource,
                &target,
            ));
            commits.push(Commit { staged, target });
        }
        outcomes.push(PackOutcome {
            original_name: child.relative_path.clone(),
            action: action.into(),
            products: products.clone(),
            causes: child.causes.clone(),
            detail: None,
            separated: products
                .into_iter()
                .map(|name| SeparatedPack {
                    name,
                    parent: shell.relative_path.clone(),
                })
                .collect(),
        });
    }

    let mut attachment_index = 0;
    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&work_root);
                return failed(error.to_string());
            }
        };
        if entry.is_dir() {
            continue;
        }
        let inner_path = decode_entry_name(entry.name_raw()).replace('\\', "/");
        if inner_path.split('/').any(is_junk_name)
            || pack_sources.iter().any(|(source, is_folder)| {
                inner_path == *source
                    || (*is_folder && inner_path.starts_with(&format!("{source}/")))
            })
        {
            continue;
        }
        let Some(relative_target) = archive_attachment_target(shell, &inner_path, &by_path) else {
            continue;
        };
        let mut bytes = Vec::new();
        if let Err(error) = entry.read_to_end(&mut bytes) {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
        let Some(file_name) = relative_target.file_name() else {
            continue;
        };
        let target_dir = targets
            .resource
            .join(relative_target.parent().unwrap_or(Path::new("")));
        let target = unique_target_reserved(
            &target_dir,
            &file_name.to_string_lossy(),
            &resource_reserved,
        );
        resource_reserved.push(target.clone());
        let staged = work_root.join(format!("attachment-{attachment_index}"));
        attachment_index += 1;
        if let Err(error) = std::fs::write(&staged, bytes) {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
        commits.push(Commit { staged, target });
    }

    let backup = match move_into(shell_src, targets.problematic) {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return vec![locked_or_failed_outcome(shell, &error)];
        }
    };
    let mut committed: Vec<std::path::PathBuf> = Vec::new();
    let mut created_dirs = Vec::new();
    for commit in &commits {
        if let Some(parent) = commit.target.parent() {
            let mut missing = Vec::new();
            let mut cursor = parent;
            while !cursor.exists() {
                missing.push(cursor.to_path_buf());
                let Some(next) = cursor.parent() else {
                    break;
                };
                cursor = next;
            }
            if let Err(error) = std::fs::create_dir_all(parent) {
                for path in committed {
                    remove_path(&path);
                }
                for path in created_dirs.iter().rev() {
                    let _ = std::fs::remove_dir(path);
                }
                let rollback = move_exact(&backup, shell_src);
                let _ = std::fs::remove_dir_all(&work_root);
                let detail = match rollback {
                    Ok(()) => error.to_string(),
                    Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
                };
                return failed(detail);
            }
            missing.reverse();
            created_dirs.extend(missing);
        }
        if let Err(error) = move_exact(&commit.staged, &commit.target) {
            for path in committed {
                remove_path(&path);
            }
            for path in created_dirs.iter().rev() {
                let _ = std::fs::remove_dir(path);
            }
            let rollback = move_exact(&backup, shell_src);
            let _ = std::fs::remove_dir_all(&work_root);
            let detail = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
            };
            return failed(detail);
        }
        committed.push(commit.target.clone());
    }
    let _ = std::fs::remove_dir_all(&work_root);
    outcomes
}

fn archive_preserved_parent(
    shell: &PackEntry,
    child: &PackEntry,
    by_path: &std::collections::HashMap<&str, &PackEntry>,
) -> std::path::PathBuf {
    let mut parts = Vec::new();
    let mut parent = child.parent_path.as_deref();
    while let Some(path) = parent {
        if path == shell.relative_path {
            break;
        }
        let Some(entry) = by_path.get(path) else {
            break;
        };
        if matches!(
            entry.kind,
            NodeKind::ClassificationFolder | NodeKind::SupportingFolder
        ) {
            parts.push(sanitize_windows_name(&entry.name));
        }
        parent = entry.parent_path.as_deref();
    }
    parts.reverse();
    parts.into_iter().collect()
}

fn archive_attachment_target(
    shell: &PackEntry,
    inner_path: &str,
    by_path: &std::collections::HashMap<&str, &PackEntry>,
) -> Option<std::path::PathBuf> {
    let components: Vec<&str> = inner_path
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    if components.len() < 2 {
        return None;
    }
    let mut current = shell.relative_path.clone();
    let mut target = std::path::PathBuf::new();
    let mut inside_preserved = false;
    for component in &components[..components.len() - 1] {
        current.push('/');
        current.push_str(component);
        match by_path.get(current.as_str()).map(|entry| entry.kind) {
            Some(NodeKind::ClassificationFolder | NodeKind::SupportingFolder) => {
                target.push(sanitize_windows_name(component));
                inside_preserved = true;
            }
            Some(NodeKind::Shell) => {}
            Some(NodeKind::Pack) => return None,
            None if inside_preserved => target.push(sanitize_windows_name(component)),
            None => {}
        }
    }
    if !inside_preserved {
        return None;
    }
    target.push(sanitize_windows_name(components.last().unwrap()));
    Some(target)
}

fn archive_shell_target_name(result_parent: &Path, base: &Path, target: &Path) -> String {
    let relative = target.strip_prefix(base).unwrap_or(target);
    (!result_parent.as_os_str().is_empty())
        .then_some(result_parent)
        .map(|parent| parent.join(relative).to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| relative.to_string_lossy().replace('\\', "/"))
}

fn remove_path(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

fn process_folder_shell(
    shell_src: &Path,
    shell: &PackEntry,
    child: &PackEntry,
    _problematic_target: &Path,
    illegal_target: &Path,
    resource_target: &Path,
    plot_temp: &Path,
) -> PackOutcome {
    if child.kind != NodeKind::Pack {
        return PackOutcome {
            original_name: shell.name.clone(),
            action: "failed".into(),
            products: Vec::new(),
            causes: shell.causes.clone(),
            detail: Some("unsupported folder shell child".into()),
            separated: Vec::new(),
        };
    }

    let child_src = shell_src.join(&child.name);
    if (child.category != Category::Normal && child.category != Category::Ignored)
        || child.causes.iter().any(|cause| cause == "lunar_escape")
    {
        return process_problem_folder_shell(
            shell_src,
            shell,
            child,
            &child_src,
            ProcessTargets {
                problematic: _problematic_target,
                illegal: illegal_target,
                resource: resource_target,
                plot_temp,
            },
        );
    }
    let target = unique_target(resource_target, &sanitize_windows_name(&child.name));
    if let Err(error) = move_exact(&child_src, &target) {
        return locked_or_failed_outcome(shell, &error);
    }
    if let Err(error) = remove_shell_junk_and_dir(shell_src) {
        let rollback = move_exact(&target, &child_src);
        let detail = match rollback {
            Ok(()) => error.to_string(),
            Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
        };
        return PackOutcome {
            original_name: shell.name.clone(),
            action: "failed".into(),
            products: Vec::new(),
            causes: shell.causes.clone(),
            detail: Some(detail),
            separated: Vec::new(),
        };
    }
    let target_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    PackOutcome {
        original_name: shell.name.clone(),
        action: "converted".into(),
        products: vec![target_name.clone()],
        causes: shell.causes.clone(),
        detail: None,
        separated: vec![SeparatedPack {
            name: target_name,
            parent: shell.relative_path.clone(),
        }],
    }
}

fn process_problem_folder_shell(
    shell_src: &Path,
    shell: &PackEntry,
    child: &PackEntry,
    child_src: &Path,
    targets: ProcessTargets<'_>,
) -> PackOutcome {
    let failed = |detail: String| PackOutcome {
        original_name: shell.name.clone(),
        action: "failed".into(),
        products: Vec::new(),
        causes: shell.causes.clone(),
        detail: Some(detail),
        separated: Vec::new(),
    };
    if child.category == Category::Illegal {
        return process_illegal_folder_shell(
            shell_src,
            shell,
            child_src,
            &child.name,
            targets.problematic,
            targets.illegal,
            targets.plot_temp,
        );
    }
    if child.category == Category::Nested
        && child.causes.iter().any(|cause| cause == "nested_container")
    {
        return process_nested_folder_shell(
            shell_src,
            shell,
            child,
            child_src,
            targets.problematic,
            targets.resource,
            targets.plot_temp,
        );
    }

    let work_root = unique_target(
        &targets.plot_temp.join(".work"),
        &format!("{}.folder-shell", sanitize_windows_name(&shell.name)),
    );
    if let Err(error) = std::fs::create_dir_all(&work_root) {
        return failed(error.to_string());
    }
    let stem = product_stem(&child.name, child_src.is_dir());
    let stage = work_root.join(format!("{stem}.zip"));
    if let Err(error) = write_fixed_zip(child_src, child_src.is_dir(), &stem, &stage) {
        let _ = std::fs::remove_dir_all(&work_root);
        return failed(error.to_string());
    }
    let target = unique_target(targets.resource, &format!("{stem}.zip"));
    let backup = match move_into(shell_src, targets.problematic) {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return locked_or_failed_outcome(shell, &error);
        }
    };
    if let Err(error) = move_exact(&stage, &target) {
        let rollback = move_exact(&backup, shell_src);
        let _ = std::fs::remove_dir_all(&work_root);
        let detail = match rollback {
            Ok(()) => error.to_string(),
            Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
        };
        return failed(detail);
    }
    let _ = std::fs::remove_dir_all(&work_root);
    let product = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    PackOutcome {
        original_name: shell.name.clone(),
        action: "converted".into(),
        products: vec![product],
        causes: shell.causes.clone(),
        detail: None,
        separated: Vec::new(),
    }
}

fn process_nested_folder_shell(
    shell_src: &Path,
    shell: &PackEntry,
    child: &PackEntry,
    child_src: &Path,
    problematic_target: &Path,
    resource_target: &Path,
    plot_temp: &Path,
) -> PackOutcome {
    let failed = |detail: String| PackOutcome {
        original_name: shell.name.clone(),
        action: "failed".into(),
        products: Vec::new(),
        causes: shell.causes.clone(),
        detail: Some(detail),
        separated: Vec::new(),
    };
    let work_root = unique_target(
        &plot_temp.join(".work"),
        &format!("{}.nested-shell", sanitize_windows_name(&shell.name)),
    );
    let staged_dir = work_root.join("products");
    let extract_work = work_root.join("extract");
    if let Err(error) = std::fs::create_dir_all(&staged_dir) {
        return failed(error.to_string());
    }
    let product_names = match unwrap_layers(child_src, &child.name, &extract_work, &staged_dir) {
        Ok(products) => products,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return failed(error.to_string());
        }
    };
    let mut reserved = Vec::new();
    let mut commits = Vec::new();
    for name in product_names {
        let target = unique_target_reserved(resource_target, &name, &reserved);
        reserved.push(target.clone());
        commits.push((staged_dir.join(&name), target));
    }
    let backup = match move_into(shell_src, problematic_target) {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return locked_or_failed_outcome(shell, &error);
        }
    };
    let mut committed: Vec<std::path::PathBuf> = Vec::new();
    for (stage, target) in &commits {
        if let Err(error) = move_exact(stage, target) {
            for path in committed {
                let _ = std::fs::remove_file(path);
            }
            let rollback = move_exact(&backup, shell_src);
            let _ = std::fs::remove_dir_all(&work_root);
            let detail = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
            };
            return failed(detail);
        }
        committed.push(target.clone());
    }
    let _ = std::fs::remove_dir_all(&work_root);
    PackOutcome {
        original_name: shell.name.clone(),
        action: "converted".into(),
        products: commits
            .into_iter()
            .map(|(_, target)| {
                target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect(),
        causes: shell.causes.clone(),
        detail: None,
        separated: Vec::new(),
    }
}

fn process_illegal_folder_shell(
    shell_src: &Path,
    shell: &PackEntry,
    child_src: &Path,
    child_name: &str,
    problematic_target: &Path,
    illegal_target: &Path,
    plot_temp: &Path,
) -> PackOutcome {
    let failed = |detail: String| PackOutcome {
        original_name: shell.name.clone(),
        action: "failed".into(),
        products: Vec::new(),
        causes: shell.causes.clone(),
        detail: Some(detail),
        separated: Vec::new(),
    };
    let work_root = unique_target(
        &plot_temp.join(".work"),
        &format!("{}.illegal-shell", sanitize_windows_name(&shell.name)),
    );
    if let Err(error) = std::fs::create_dir_all(&work_root) {
        return failed(error.to_string());
    }
    let stage = work_root.join(sanitize_windows_name(child_name));
    let prepared = if child_src.is_dir() {
        copy_dir(child_src, &stage)
    } else {
        std::fs::copy(child_src, &stage).map(|_| ())
    };
    if let Err(error) = prepared {
        let _ = std::fs::remove_dir_all(&work_root);
        return failed(error.to_string());
    }
    let target = unique_target(illegal_target, &sanitize_windows_name(child_name));
    let backup = match move_into(shell_src, problematic_target) {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work_root);
            return locked_or_failed_outcome(shell, &error);
        }
    };
    if let Err(error) = move_exact(&stage, &target) {
        let rollback = move_exact(&backup, shell_src);
        let _ = std::fs::remove_dir_all(&work_root);
        let detail = match rollback {
            Ok(()) => error.to_string(),
            Err(rollback_error) => format!("{error}; rollback failed: {rollback_error}"),
        };
        return failed(detail);
    }
    let _ = std::fs::remove_dir_all(&work_root);
    let product = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    PackOutcome {
        original_name: shell.name.clone(),
        action: "moved_to_illegal".into(),
        products: vec![product],
        causes: shell.causes.clone(),
        detail: None,
        separated: Vec::new(),
    }
}

fn remove_shell_junk_and_dir(shell: &Path) -> std::io::Result<()> {
    for item in std::fs::read_dir(shell)?.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if !is_junk_name(&name) {
            return Err(std::io::Error::other(format!(
                "folder shell gained non-junk content: {name}"
            )));
        }
        let path = item.path();
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
    }
    std::fs::remove_dir(shell)
}

fn qualify_outcome_paths(entry: &PackEntry, outcome: &mut PackOutcome) {
    outcome.original_name = entry.relative_path.clone();
    let parent = Path::new(&entry.relative_path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    let qualify = |name: &str| {
        parent
            .map(|path| path.join(name).to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| name.to_string())
    };
    outcome.products = outcome.products.iter().map(|name| qualify(name)).collect();
    for separated in &mut outcome.separated {
        separated.name = qualify(&separated.name);
        separated.parent = entry.relative_path.clone();
    }
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
        separated: Vec::new(),
    }
}

fn restore_classified_failure(
    src: &Path,
    moved: &Path,
    entry: &PackEntry,
    mut outcome: PackOutcome,
) -> PackOutcome {
    if outcome.action != "failed" || entry.parent_path.is_none() {
        return outcome;
    }
    if let Err(error) = move_exact(moved, src) {
        let detail = outcome.detail.take().unwrap_or_default();
        outcome.detail = Some(format!("{detail}; restore failed: {error}"));
    }
    outcome
}

fn move_exact(src: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("restore target already exists: {}", target.display()),
        ));
    }
    if std::fs::rename(src, target).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        copy_dir(src, target)?;
        if let Err(error) = std::fs::remove_dir_all(src) {
            let _ = std::fs::remove_dir_all(target);
            return Err(error);
        }
    } else {
        std::fs::copy(src, target)?;
        if let Err(error) = std::fs::remove_file(src) {
            let _ = std::fs::remove_file(target);
            return Err(error);
        }
    }
    Ok(())
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
            separated: Vec::new(),
        },
        Err(e) => restore_classified_failure(
            src,
            &moved,
            entry,
            PackOutcome {
                original_name: entry.name.clone(),
                action: "failed".into(),
                products: Vec::new(),
                causes: entry.causes.clone(),
                detail: Some(e.to_string()),
                separated: Vec::new(),
            },
        ),
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
        let decoded = decode_entry_name(entry.name_raw()).replace('\\', "/");
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
    plot_temp: &Path,
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
                separated: Vec::new(),
            };
        }
    };
    match collect_misplaced_containers(&moved) {
        Ok(containers) if !containers.is_empty() => {
            let outcome =
                rebuild_with_separations(&moved, entry, resourcepacks, plot_temp, containers);
            return restore_classified_failure(src, &moved, entry, outcome);
        }
        Err(error) => {
            return restore_classified_failure(
                src,
                &moved,
                entry,
                PackOutcome {
                    original_name: entry.name.clone(),
                    action: "failed".into(),
                    products: Vec::new(),
                    causes: entry.causes.clone(),
                    detail: Some(error.to_string()),
                    separated: Vec::new(),
                },
            );
        }
        Ok(_) => {}
    }
    match build_root_fixed_product(&moved, &entry.name, resourcepacks) {
        Ok(product) => PackOutcome {
            original_name: entry.name.clone(),
            action: "converted".into(),
            products: vec![product],
            causes: entry.causes.clone(),
            detail: None,
            separated: Vec::new(),
        },
        Err(e) => restore_classified_failure(
            src,
            &moved,
            entry,
            PackOutcome {
                original_name: entry.name.clone(),
                action: "failed".into(),
                products: Vec::new(),
                causes: entry.causes.clone(),
                detail: Some(e.to_string()),
                separated: Vec::new(),
            },
        ),
    }
}

#[derive(Debug)]
enum MisplacedContainer {
    File { name: String, bytes: Vec<u8> },
    Folder { name: String, source: FolderSource },
}

#[derive(Debug)]
enum FolderSource {
    Path(std::path::PathBuf),
    Entries(Vec<(String, Vec<u8>)>),
}

fn collect_misplaced_containers(moved: &Path) -> std::io::Result<Vec<MisplacedContainer>> {
    if moved.is_dir() {
        let mut out = Vec::new();
        for item in std::fs::read_dir(moved)?.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if is_junk_name(&name) || name == "assets" || name == "pack.mcmeta" {
                continue;
            }
            let path = item.path();
            if path.is_dir() {
                if folder_contains_core(&path, 1) {
                    out.push(MisplacedContainer::Folder {
                        name,
                        source: FolderSource::Path(path),
                    });
                }
            } else {
                let bytes = std::fs::read(&path)?;
                if is_container_payload(&name, &bytes) {
                    out.push(MisplacedContainer::File { name, bytes });
                }
            }
        }
        return Ok(out);
    }

    let file = std::fs::File::open(moved)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        names.push(decode_entry_name(entry.name_raw()).replace('\\', "/"));
    }
    let mut out = Vec::new();
    let mut root_dirs = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let decoded = decode_entry_name(entry.name_raw()).replace('\\', "/");
        if let Some((root, _)) = decoded.split_once('/') {
            if !root.is_empty() && !is_junk_name(root) && root != "assets" {
                root_dirs.insert(root.to_string());
            }
            continue;
        }
        if is_junk_name(&decoded)
            || decoded == "pack.mcmeta"
            || decoded == "pack.png"
            || is_mcmeta_variant(&decoded)
            || is_png_variant(&decoded)
        {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if is_container_payload(&decoded, &bytes) {
            out.push(MisplacedContainer::File {
                name: decoded,
                bytes,
            });
        }
    }
    for root in root_dirs {
        let prefix = format!("{root}/");
        if zip_prefix_contains_core(&mut archive, &names, &prefix, 1) {
            out.push(MisplacedContainer::Folder {
                name: root,
                source: FolderSource::Entries(read_zip_folder_entries(&mut archive, &prefix)?),
            });
        }
    }
    Ok(out)
}

fn read_zip_folder_entries<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    prefix: &str,
) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let decoded = decode_entry_name(entry.name_raw()).replace('\\', "/");
        let Some(relative) = decoded.strip_prefix(prefix) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.push((relative.to_string(), bytes));
    }
    Ok(files)
}

fn is_container_payload(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    bytes.starts_with(b"PK")
        || bytes.starts_with(b"Rar!")
        || bytes.starts_with(b"7z\xbc\xaf\x27\x1c")
        || lower.ends_with(".zip")
        || lower.ends_with(".rar")
        || lower.ends_with(".7z")
}

fn folder_contains_core(dir: &Path, depth: usize) -> bool {
    if depth > MAX_NESTING_DEPTH {
        return false;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    for item in rd.flatten() {
        let path = item.path();
        if path.is_dir() {
            if item.file_name().to_string_lossy() == "assets" {
                return true;
            }
            if folder_contains_core(&path, depth + 1) {
                return true;
            }
        } else if let Ok(bytes) = std::fs::read(&path) {
            if bytes.starts_with(b"PK") {
                if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) {
                    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
                    if names.iter().any(|name| name.starts_with("assets/")) {
                        return true;
                    }
                    let mut search = CoreSearch::default();
                    search_zip(&mut archive, &names, "", depth + 1, &mut search);
                    if !search.found.is_empty() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn zip_prefix_contains_core<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
    prefix: &str,
    depth: usize,
) -> bool {
    if depth > MAX_NESTING_DEPTH {
        return false;
    }
    if names
        .iter()
        .any(|name| name.starts_with(&format!("{prefix}assets/")))
    {
        return true;
    }
    for dir in zip_child_dirs(names, prefix) {
        if zip_prefix_contains_core(archive, names, &format!("{prefix}{dir}/"), depth + 1) {
            return true;
        }
    }
    for file_name in zip_child_files(names, prefix) {
        let full = format!("{prefix}{file_name}");
        let mut bytes = Vec::new();
        {
            let Ok(mut entry) = archive.by_name(&full) else {
                continue;
            };
            if entry.read_to_end(&mut bytes).is_err() || !bytes.starts_with(b"PK") {
                continue;
            }
        }
        let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(bytes)) else {
            continue;
        };
        let inner_names: Vec<String> = inner.file_names().map(str::to_string).collect();
        if zip_prefix_contains_core(&mut inner, &inner_names, "", depth + 1) {
            return true;
        }
    }
    false
}

fn rebuild_with_separations(
    moved: &Path,
    entry: &PackEntry,
    resourcepacks: &Path,
    plot_temp: &Path,
    containers: Vec<MisplacedContainer>,
) -> PackOutcome {
    let work_root = unique_target(
        &plot_temp.join(".work"),
        &format!("{}.separate", file_stem_of(&entry.name)),
    );
    let fail = |error: std::io::Error| PackOutcome {
        original_name: entry.name.clone(),
        action: "failed".into(),
        products: Vec::new(),
        causes: entry.causes.clone(),
        detail: Some(error.to_string()),
        separated: Vec::new(),
    };
    if let Err(error) = std::fs::create_dir_all(&work_root) {
        return fail(error);
    }
    let was_dir = moved.is_dir();
    let stem = product_stem(&entry.name, was_dir);
    let parent_final = unique_target(resourcepacks, &format!("{stem}.zip"));
    let parent_tmp = work_root.join("parent.zip.partial");
    if let Err(error) = write_fixed_zip(moved, was_dir, &stem, &parent_tmp) {
        let _ = std::fs::remove_dir_all(&work_root);
        return fail(error);
    }

    struct Staged {
        target: std::path::PathBuf,
        staged: std::path::PathBuf,
        name: String,
    }
    let mut staged = Vec::new();
    let mut reserved = vec![parent_final.clone()];
    for (index, container) in containers.into_iter().enumerate() {
        let raw_name = match &container {
            MisplacedContainer::File { name, .. } | MisplacedContainer::Folder { name, .. } => name,
        };
        let safe_name = sanitize_windows_name(raw_name);
        let target = unique_target_reserved(resourcepacks, &safe_name, &reserved);
        reserved.push(target.clone());
        let stage = work_root.join(format!("child-{index}"));
        let result = match container {
            MisplacedContainer::File { bytes, .. } => std::fs::write(&stage, bytes),
            MisplacedContainer::Folder { source, .. } => match source {
                FolderSource::Path(path) => copy_dir(&path, &stage),
                FolderSource::Entries(files) => write_staged_folder(&stage, files),
            },
        };
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&work_root);
            return fail(error);
        }
        staged.push(Staged {
            target,
            staged: stage,
            name: safe_name,
        });
    }

    if let Err(error) = std::fs::rename(&parent_tmp, &parent_final) {
        let _ = std::fs::remove_dir_all(&work_root);
        return fail(error);
    }
    let mut committed: Vec<std::path::PathBuf> = Vec::new();
    for item in &staged {
        if let Err(error) = std::fs::rename(&item.staged, &item.target) {
            let _ = std::fs::remove_file(&parent_final);
            for path in committed {
                let _ = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
            }
            let _ = std::fs::remove_dir_all(&work_root);
            return fail(error);
        }
        committed.push(item.target.clone());
    }
    let _ = std::fs::remove_dir_all(&work_root);
    PackOutcome {
        original_name: entry.name.clone(),
        action: "converted".into(),
        products: vec![parent_final
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()],
        causes: entry.causes.clone(),
        detail: None,
        separated: staged
            .into_iter()
            .map(|item| SeparatedPack {
                name: item
                    .target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(item.name),
                parent: entry.name.clone(),
            })
            .collect(),
    }
}

fn write_staged_folder(stage: &Path, files: Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
    std::fs::create_dir_all(stage)?;
    for (relative, bytes) in files {
        let mut target = stage.to_path_buf();
        for component in relative.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                return Err(std::io::Error::other("unsafe embedded folder path"));
            }
            target.push(sanitize_windows_name(component));
        }
        if target == stage {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

fn unique_target_reserved(
    dir: &Path,
    name: &str,
    reserved: &[std::path::PathBuf],
) -> std::path::PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists()
        && !reserved
            .iter()
            .any(|path| same_windows_target(path, &candidate))
    {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1.. {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists()
            && !reserved
                .iter()
                .any(|path| same_windows_target(path, &candidate))
        {
            return candidate;
        }
    }
    unreachable!()
}

fn unique_target_reserved_ignoring(
    dir: &Path,
    name: &str,
    reserved: &[std::path::PathBuf],
    ignored_existing: &Path,
) -> std::path::PathBuf {
    let available = |candidate: &Path| {
        (!candidate.exists() || same_windows_target(candidate, ignored_existing))
            && !reserved
                .iter()
                .any(|path| same_windows_target(path, candidate))
    };
    let candidate = dir.join(name);
    if available(&candidate) {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    for index in 1.. {
        let candidate = dir.join(format!("{stem} ({index}){ext}"));
        if available(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn same_windows_target(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
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
        let mut candidates: Vec<&String> = names
            .iter()
            .filter(|n| !n.contains('/') && is_mcmeta_variant(n))
            .collect();
        candidates.sort_by(|a, b| mcmeta_candidate_cmp(a, b));
        candidates.first().copied()
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
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| std::io::Error::other(e.to_string()))?;
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
fn collect_relative_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
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

/// Resolves this batch's run folder inside plot_temp. The timestamp name
/// carries no meaningful extension, so the " (n)" suffix goes at the end.
fn unique_run_root(plot_temp: &Path, base: &str) -> std::path::PathBuf {
    let candidate = plot_temp.join(base);
    if !candidate.exists() {
        return candidate;
    }
    for n in 1.. {
        let candidate = plot_temp.join(format!("{base} ({n})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
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
        if let Err(error) = std::fs::remove_dir_all(src) {
            let _ = std::fs::remove_dir_all(&target);
            return Err(error);
        }
    } else {
        std::fs::copy(src, &target)?;
        if let Err(error) = std::fs::remove_file(src) {
            let _ = std::fs::remove_file(&target);
            return Err(error);
        }
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
    let bytes = std::fs::read(path).unwrap_or_default();
    classify_file_bytes(&bytes, name)
}

fn classify_file_bytes(bytes: &[u8], name: &str) -> (Category, Vec<String>) {
    let n = bytes.len().min(6);
    let magic = &bytes[..n];
    if n < 2 || &magic[..2] != b"PK" {
        let cause = if n >= 4 && &magic[..4] == b"Rar!" {
            "rar_archive"
        } else if n >= 6 && magic == b"7z\xbc\xaf\x27\x1c" {
            "sevenz_archive"
        } else {
            "not_zip"
        };
        return (Category::Illegal, vec![cause.into()]);
    }
    let mut archive = match zip::ZipArchive::new(Cursor::new(bytes)) {
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
