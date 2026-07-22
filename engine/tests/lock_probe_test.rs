#![cfg(windows)]
mod common;

use common::{core_entries, make_zip};
use engine::probe_locked;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;

/// How MC (Java) holds the selected pack: read/write shared, delete denied.
fn hold(path: &std::path::Path) -> fs::File {
    fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(path)
        .unwrap()
}

#[test]
fn a_held_zip_is_reported_a_free_zip_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    make_zip(&rp.join("InUse.zip"), &core_entries());
    make_zip(&rp.join("Free.zip"), &core_entries());
    let names: Vec<String> = vec!["InUse.zip".into(), "Free.zip".into()];

    let lock = hold(&rp.join("InUse.zip"));
    assert_eq!(probe_locked(&rp, &names), vec!["InUse.zip".to_string()]);

    drop(lock);
    assert!(probe_locked(&rp, &names).is_empty());
}

#[test]
fn folder_packs_never_join_the_precheck() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    let folder = rp.join("FolderPack");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("pack.mcmeta"), b"{}").unwrap();

    let _lock = hold(&folder.join("pack.mcmeta"));
    assert!(probe_locked(&rp, &["FolderPack".to_string()]).is_empty());
}
