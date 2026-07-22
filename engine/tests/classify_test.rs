mod common;

use common::{core_entries, make_zip};
use engine::{scan, Category};
use std::fs;

#[test]
fn a_file_without_zip_magic_is_illegal() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("note.txt"), b"just some text").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Illegal);
    assert!(entry.causes.iter().any(|c| c == "not_zip"));
    assert_eq!(report.counts.illegal, 1);
}

#[test]
fn zip_content_with_a_wrong_extension_is_nested_and_rescuable() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(&tmp.path().join("upper.ZIP"), &core_entries());
    make_zip(&tmp.path().join("fake.rar"), &core_entries());
    make_zip(&tmp.path().join("noext"), &core_entries());

    let report = scan(tmp.path());

    for entry in &report.entries {
        assert_eq!(entry.category, Category::Nested, "entry {}", entry.name);
        assert!(entry.causes.iter().any(|c| c == "wrong_extension"));
    }
    assert_eq!(report.counts.nested, 3);
}

#[test]
fn real_rar_and_sevenz_archives_are_illegal_with_specific_causes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("pack.rar"), b"Rar!\x1a\x07\x01\x00rest").unwrap();
    fs::write(tmp.path().join("pack.7z"), b"7z\xbc\xaf\x27\x1crest").unwrap();

    let report = scan(tmp.path());

    let rar = report.entries.iter().find(|e| e.name == "pack.rar").unwrap();
    let sevenz = report.entries.iter().find(|e| e.name == "pack.7z").unwrap();
    assert_eq!(rar.category, Category::Illegal);
    assert!(rar.causes.iter().any(|c| c == "rar_archive"));
    assert_eq!(sevenz.category, Category::Illegal);
    assert!(sevenz.causes.iter().any(|c| c == "sevenz_archive"));
}

#[test]
fn junk_files_inside_a_zip_do_not_trigger_bloated() {
    let tmp = tempfile::tempdir().unwrap();
    let mut entries = core_entries();
    entries.push(("Thumbs.db", b"junk" as &[u8]));
    entries.push(("assets/minecraft/.DS_Store", b"junk"));
    make_zip(&tmp.path().join("clean.zip"), &entries);

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Normal);
}

#[test]
fn a_zip_with_the_records_dead_path_is_bloated() {
    let tmp = tempfile::tempdir().unwrap();
    let mut entries = core_entries();
    entries.push(("assets/minecraft/records/cat.ogg", b"oggdata" as &[u8]));
    make_zip(&tmp.path().join("yokabi.zip"), &entries);

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Bloated);
    assert!(entry.causes.iter().any(|c| c == "dead_path"));
    assert_eq!(report.counts.bloated, 1);
}

#[test]
fn png_case_or_typo_variants_are_bloated_and_rescuable() {
    let tmp = tempfile::tempdir().unwrap();
    for (i, png_name) in ["Pack.png", "pack..png", "pack.png.png"].iter().enumerate() {
        let entries: Vec<(&str, &[u8])> = vec![
            ("pack.mcmeta", common::MCMETA),
            (png_name, b"png"),
            ("assets/minecraft/textures/a.png", b"png"),
        ];
        make_zip(&tmp.path().join(format!("p{i}.zip")), &entries);
    }

    let report = scan(tmp.path());

    for entry in &report.entries {
        assert_eq!(entry.category, Category::Bloated, "entry {}", entry.name);
        assert!(entry.causes.iter().any(|c| c == "png_rescue"), "entry {}", entry.name);
    }
}

#[test]
fn mcmeta_case_typo_or_missing_with_assets_is_nested_and_rescuable() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: Vec<(&str, Vec<(&str, &[u8])>)> = vec![
        ("case.zip", vec![("Pack.mcmeta", common::MCMETA), ("assets/minecraft/a.png", b"x")]),
        ("typo.zip", vec![("pack.mcmeta.mcmeta", common::MCMETA), ("assets/minecraft/a.png", b"x")]),
        ("missing.zip", vec![("assets/minecraft/a.png", b"x")]),
    ];
    for (name, entries) in &cases {
        make_zip(&tmp.path().join(name), entries);
    }

    let report = scan(tmp.path());

    let by_name = |n: &str| report.entries.iter().find(|e| e.name == n).unwrap();
    assert_eq!(by_name("case.zip").category, Category::Nested);
    assert!(by_name("case.zip").causes.iter().any(|c| c == "mcmeta_rescue"));
    assert_eq!(by_name("typo.zip").category, Category::Nested);
    assert!(by_name("typo.zip").causes.iter().any(|c| c == "mcmeta_rescue"));
    assert_eq!(by_name("missing.zip").category, Category::Nested);
    assert!(by_name("missing.zip").causes.iter().any(|c| c == "mcmeta_rescue"));
}

#[test]
fn a_folder_with_three_core_files_is_folder_category() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("MyPack");
    fs::create_dir_all(pack.join("assets/minecraft/textures")).unwrap();
    fs::write(pack.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(pack.join("pack.png"), b"png").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Folder);
    assert_eq!(report.counts.folder, 1);
}

#[test]
fn a_folder_pack_with_extras_stays_folder_but_lists_the_extra_cause() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("MyPack");
    fs::create_dir_all(pack.join("assets/minecraft")).unwrap();
    fs::write(pack.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(pack.join("credits.txt"), b"by me").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Folder);
    assert!(entry.causes.iter().any(|c| c == "root_extras"));
}

#[test]
fn a_zip_wrapping_a_pack_folder_is_nested() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("wrapped.zip"),
        &[
            ("Cool Pack/pack.mcmeta", common::MCMETA),
            ("Cool Pack/assets/minecraft/a.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Nested);
    assert!(entry.causes.iter().any(|c| c == "nested_container"));
}

#[test]
fn a_folder_wrapping_a_pack_folder_is_nested() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp.path().join("Wrapper/Cool Pack");
    fs::create_dir_all(inner.join("assets/minecraft")).unwrap();
    fs::write(inner.join("pack.mcmeta"), common::MCMETA).unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Nested);
    assert!(entry.causes.iter().any(|c| c == "nested_container"));
}

#[test]
fn a_zip_containing_a_pack_zip_is_nested() {
    let tmp = tempfile::tempdir().unwrap();
    let inner_path = tmp.path().join("inner.zip");
    make_zip(&inner_path, &core_entries());
    let inner_bytes = fs::read(&inner_path).unwrap();
    fs::remove_file(&inner_path).unwrap();
    make_zip(
        &tmp.path().join("outer.zip"),
        &[("Real Pack.zip", inner_bytes.as_slice())],
    );

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Nested);
    assert!(entry.causes.iter().any(|c| c == "nested_container"));
}

#[test]
fn a_collection_zip_with_sibling_packs_is_nested() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("bundle.zip"),
        &[
            ("PackA/pack.mcmeta", common::MCMETA),
            ("PackA/assets/minecraft/a.png", b"x"),
            ("PackB/pack.mcmeta", common::MCMETA),
            ("PackB/assets/minecraft/b.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Nested);
}

#[test]
fn an_empty_zip_is_illegal() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(&tmp.path().join("empty.zip"), &[]);

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Illegal);
    assert!(entry.causes.iter().any(|c| c == "no_core_found"));
}

#[test]
fn nesting_deeper_than_ten_levels_is_illegal_too_deep() {
    let tmp = tempfile::tempdir().unwrap();
    let deep = "a/".repeat(11);
    let mcmeta_path = format!("{deep}pack.mcmeta");
    let asset_path = format!("{deep}assets/minecraft/a.png");
    make_zip(
        &tmp.path().join("deep.zip"),
        &[(mcmeta_path.as_str(), common::MCMETA), (asset_path.as_str(), b"x")],
    );

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Illegal);
    assert!(entry.causes.iter().any(|c| c == "too_deep"));
}

#[test]
fn an_encrypted_zip_is_illegal() {
    let tmp = tempfile::tempdir().unwrap();
    common::make_encrypted_zip(&tmp.path().join("locked.zip"), &core_entries());

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Illegal);
    assert!(entry.causes.iter().any(|c| c == "encrypted_zip"));
}

#[test]
fn entries_carry_their_size_in_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(&tmp.path().join("pack.zip"), &core_entries());
    let pack_dir = tmp.path().join("FolderPack");
    fs::create_dir_all(pack_dir.join("assets/minecraft")).unwrap();
    fs::write(pack_dir.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(pack_dir.join("assets/minecraft/a.bin"), vec![0u8; 1000]).unwrap();

    let report = scan(tmp.path());

    let zip_entry = report.entries.iter().find(|e| e.name == "pack.zip").unwrap();
    let dir_entry = report.entries.iter().find(|e| e.name == "FolderPack").unwrap();
    let zip_disk = fs::metadata(tmp.path().join("pack.zip")).unwrap().len();
    assert_eq!(zip_entry.size_bytes, zip_disk);
    assert_eq!(
        dir_entry.size_bytes,
        1000 + common::MCMETA.len() as u64
    );
}

#[test]
fn plot_temp_and_excluded_paths_are_invisible_to_scan() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("plot_temp/problematic_packs")).unwrap();
    fs::write(tmp.path().join("Plot.exe"), b"MZfake").unwrap();
    make_zip(&tmp.path().join("real.zip"), &core_entries());

    let opts = engine::ScanOptions {
        exclude: vec![tmp.path().join("Plot.exe")],
    };
    let report = engine::scan_with(tmp.path(), &opts);

    let names: Vec<_> = report.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["real.zip"]);
}

#[test]
fn a_nested_pack_with_stray_root_files_is_still_nested() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("wrapped.zip"),
        &[
            ("readme.txt", b"stray" as &[u8]),
            ("Cool Pack/pack.mcmeta", common::MCMETA),
            ("Cool Pack/assets/minecraft/a.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Nested);
}

#[test]
fn a_zip_with_exactly_the_three_core_files_is_normal() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(&tmp.path().join("Pack.zip"), &core_entries());

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Normal);
    assert!(entry.causes.is_empty());
    assert_eq!(report.counts.normal, 1);
}
