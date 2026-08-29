mod common;

use common::{core_entries, make_zip, MCMETA};
use engine::{scan, scan_with_progress, ScanOptions, ScanStatus};
use std::fs;
use std::sync::Mutex;

#[test]
fn scanning_a_missing_directory_reports_missing_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("resourcepacks");

    let report = scan(&missing);

    assert_eq!(report.status, ScanStatus::MissingDir);
    assert_eq!(report.entries.len(), 0);
}

#[test]
fn a_directory_with_only_junk_files_reports_no_packs() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("desktop.ini"), b"junk").unwrap();
    fs::create_dir(tmp.path().join("__MACOSX")).unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.status, ScanStatus::NoPacks);
    assert_eq!(report.entries.len(), 0);
}

#[test]
fn scan_lists_top_level_entries_excluding_junk() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("PackA.zip"), b"stub").unwrap();
    fs::create_dir(tmp.path().join("PackB")).unwrap();
    fs::write(tmp.path().join("desktop.ini"), b"junk").unwrap();
    fs::write(tmp.path().join("Thumbs.db"), b"junk").unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.status, ScanStatus::Ok);
    let mut names: Vec<_> = report.entries.iter().map(|e| e.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["PackA.zip", "PackB"]);
}

#[test]
fn parallel_scan_reports_progress_and_matches_serial_scan() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(&tmp.path().join("A.zip"), &core_entries());
    make_zip(
        &tmp.path().join("B.zip"),
        &[
            ("wrapper/pack.mcmeta", MCMETA),
            ("wrapper/assets/minecraft/x.png", b"png".as_slice()),
        ],
    );
    fs::write(tmp.path().join("C.txt"), b"not a zip").unwrap();

    let events: Mutex<Vec<(String, usize, usize)>> = Mutex::new(Vec::new());
    let report = scan_with_progress(tmp.path(), &ScanOptions::default(), &|p| {
        events
            .lock()
            .unwrap()
            .push((p.name.clone(), p.index, p.total));
    });

    // classification identical to the serial scan
    let serial = scan(tmp.path());
    let key = |r: &engine::ScanReport| {
        let mut v: Vec<(String, String)> = r
            .entries
            .iter()
            .map(|e| (e.name.clone(), format!("{:?}", e.category)))
            .collect();
        v.sort();
        v
    };
    assert_eq!(key(&report), key(&serial));

    // one event per pack: totals all 3, completed counts are 1..=3, names cover all
    let ev = events.into_inner().unwrap();
    assert_eq!(ev.len(), 3);
    assert!(ev.iter().all(|(_, _, total)| *total == 3));
    let mut indexes: Vec<usize> = ev.iter().map(|e| e.1).collect();
    indexes.sort();
    assert_eq!(indexes, vec![1, 2, 3]);
    let mut names: Vec<&str> = ev.iter().map(|e| e.0.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["A.zip", "B.zip", "C.txt"]);
}
