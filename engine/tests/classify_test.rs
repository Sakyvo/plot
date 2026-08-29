mod common;

use common::{core_entries, make_zip};
use engine::{scan, Category, NodeKind};
use std::fs;

#[test]
fn a_folder_with_two_direct_packs_is_a_classification_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let category = tmp.path().join("PotPvP");
    fs::create_dir_all(&category).unwrap();
    make_zip(&category.join("A.zip"), &core_entries());
    make_zip(&category.join("B.zip"), &core_entries());

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 2);
    assert_eq!(report.counts.folder, 1);
    assert_eq!(report.counts.normal, 2);
    let parent = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "PotPvP")
        .unwrap();
    assert_eq!(parent.kind, NodeKind::ClassificationFolder);
    assert_eq!(parent.category, Category::Folder);
    assert_eq!(parent.parent_path, None);
    for name in ["A.zip", "B.zip"] {
        let relative_path = format!("PotPvP/{name}");
        let child = report
            .entries
            .iter()
            .find(|entry| entry.relative_path == relative_path)
            .unwrap();
        assert_eq!(child.kind, NodeKind::Pack);
        assert_eq!(child.category, Category::Normal);
        assert_eq!(child.parent_path.as_deref(), Some("PotPvP"));
    }
}

#[test]
fn nested_classification_folders_preserve_the_full_tree() {
    let tmp = tempfile::tempdir().unwrap();
    for (folder, packs) in [("Melee", ["A.zip", "B.zip"]), ("UHC", ["C.zip", "D.zip"])] {
        let path = tmp.path().join("PotPvP").join(folder);
        fs::create_dir_all(&path).unwrap();
        for pack in packs {
            make_zip(&path.join(pack), &core_entries());
        }
    }

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 4);
    assert_eq!(report.counts.folder, 3);
    let node = |relative_path: &str| {
        report
            .entries
            .iter()
            .find(|entry| entry.relative_path == relative_path)
            .unwrap()
    };
    assert_eq!(node("PotPvP").kind, NodeKind::ClassificationFolder);
    assert_eq!(node("PotPvP").parent_path, None);
    for folder in ["Melee", "UHC"] {
        let relative_path = format!("PotPvP/{folder}");
        assert_eq!(node(&relative_path).kind, NodeKind::ClassificationFolder);
        assert_eq!(node(&relative_path).parent_path.as_deref(), Some("PotPvP"));
    }
    for (folder, packs) in [("Melee", ["A.zip", "B.zip"]), ("UHC", ["C.zip", "D.zip"])] {
        for pack in packs {
            let relative_path = format!("PotPvP/{folder}/{pack}");
            assert_eq!(node(&relative_path).kind, NodeKind::Pack);
            assert_eq!(
                node(&relative_path).parent_path.as_deref(),
                Some(format!("PotPvP/{folder}").as_str())
            );
        }
    }
}

#[test]
fn a_single_pack_with_an_attachment_keeps_a_non_problem_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("Download");
    fs::create_dir_all(&folder).unwrap();
    make_zip(&folder.join("A.zip"), &core_entries());
    fs::write(folder.join("preview.png"), b"preview").unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 1);
    assert_eq!(report.counts.folder, 0);
    assert_eq!(report.counts.nested, 0);
    assert_eq!(report.counts.normal, 1);
    let parent = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Download")
        .unwrap();
    assert_eq!(parent.kind, NodeKind::SupportingFolder);
    let child = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Download/A.zip")
        .unwrap();
    assert_eq!(child.kind, NodeKind::Pack);
    assert_eq!(child.category, Category::Normal);
    assert_eq!(child.parent_path.as_deref(), Some("Download"));
    assert!(report
        .entries
        .iter()
        .all(|entry| entry.name != "preview.png"));
}

#[test]
fn a_single_pack_folder_without_attachments_is_a_shell_with_one_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let wrapper = tmp.path().join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    make_zip(&wrapper.join("A.zip"), &core_entries());

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 1);
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.normal, 1);
    assert_eq!(report.entries.len(), 2);
    let shell = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Wrapper")
        .unwrap();
    assert_eq!(shell.kind, NodeKind::Shell);
    assert_eq!(shell.category, Category::Nested);
    assert!(shell.causes.iter().any(|cause| cause == "folder_shell"));
    let child = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Wrapper/A.zip")
        .unwrap();
    assert_eq!(child.kind, NodeKind::Pack);
    assert_eq!(child.category, Category::Normal);
    assert_eq!(child.parent_path.as_deref(), Some("Wrapper"));
}

#[test]
fn broken_archives_count_as_classification_children_but_attachments_do_not() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("Mixed");
    fs::create_dir_all(folder.join("notes")).unwrap();
    fs::write(folder.join("broken.zip"), b"broken").unwrap();
    fs::write(folder.join("encrypted.RAR"), b"broken").unwrap();
    fs::write(folder.join("damaged.7z"), b"broken").unwrap();
    fs::write(folder.join("readme.txt"), b"read me").unwrap();
    fs::write(folder.join("preview.png"), b"preview").unwrap();
    fs::write(folder.join("notes/about.txt"), b"notes").unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 3);
    assert_eq!(report.counts.folder, 1);
    assert_eq!(report.counts.illegal, 3);
    assert_eq!(report.entries.len(), 4);
    assert_eq!(report.entries[0].kind, NodeKind::ClassificationFolder);
    for name in ["broken.zip", "encrypted.RAR", "damaged.7z"] {
        let child = report
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap();
        assert_eq!(child.kind, NodeKind::Pack);
        assert_eq!(child.category, Category::Illegal);
        assert_eq!(child.parent_path.as_deref(), Some("Mixed"));
    }
}

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

    let rar = report
        .entries
        .iter()
        .find(|e| e.name == "pack.rar")
        .unwrap();
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
        assert!(
            entry.causes.iter().any(|c| c == "png_rescue"),
            "entry {}",
            entry.name
        );
    }
}

#[test]
fn mcmeta_case_typo_or_missing_with_assets_is_nested_and_rescuable() {
    let tmp = tempfile::tempdir().unwrap();
    let cases = vec![
        (
            "case.zip",
            vec![
                ("Pack.mcmeta", common::MCMETA),
                ("assets/minecraft/a.png", b"x"),
            ],
        ),
        (
            "typo.zip",
            vec![
                ("pack.mcmeta.mcmeta", common::MCMETA),
                ("assets/minecraft/a.png", b"x"),
            ],
        ),
        ("missing.zip", vec![("assets/minecraft/a.png", b"x")]),
    ];
    for (name, entries) in &cases {
        make_zip(&tmp.path().join(name), entries);
    }

    let report = scan(tmp.path());

    let by_name = |n: &str| report.entries.iter().find(|e| e.name == n).unwrap();
    assert_eq!(by_name("case.zip").category, Category::Nested);
    assert!(by_name("case.zip")
        .causes
        .iter()
        .any(|c| c == "mcmeta_rescue"));
    assert_eq!(by_name("typo.zip").category, Category::Nested);
    assert!(by_name("typo.zip")
        .causes
        .iter()
        .any(|c| c == "mcmeta_rescue"));
    assert_eq!(by_name("missing.zip").category, Category::Nested);
    assert!(by_name("missing.zip")
        .causes
        .iter()
        .any(|c| c == "mcmeta_rescue"));
}

#[test]
fn a_folder_pack_is_bloated_instead_of_a_classification_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("MyPack");
    fs::create_dir_all(pack.join("assets/minecraft/textures")).unwrap();
    fs::write(pack.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(pack.join("pack.png"), b"png").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Bloated);
    assert!(entry.causes.iter().any(|cause| cause == "folder_pack"));
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.folder, 0);
}

#[test]
fn a_folder_pack_with_extras_is_bloated_and_lists_both_causes() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("MyPack");
    fs::create_dir_all(pack.join("assets/minecraft")).unwrap();
    fs::write(pack.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(pack.join("credits.txt"), b"by me").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Bloated);
    assert!(entry.causes.iter().any(|c| c == "folder_pack"));
    assert!(entry.causes.iter().any(|c| c == "root_extras"));
}

#[test]
fn a_zip_wrapping_a_pack_folder_is_an_archive_shell_with_a_folder_pack() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("wrapped.zip"),
        &[
            ("Cool Pack/pack.mcmeta", common::MCMETA),
            ("Cool Pack/assets/minecraft/a.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].kind, NodeKind::Shell);
    assert!(report.entries[0]
        .causes
        .iter()
        .any(|cause| cause == "archive_shell"));
    assert_eq!(report.entries[1].category, Category::Bloated);
    assert!(report.entries[1]
        .causes
        .iter()
        .any(|cause| cause == "folder_pack"));
}

#[test]
fn a_folder_wrapping_a_pack_folder_is_a_shell_with_a_bloated_child() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp.path().join("Wrapper/Cool Pack");
    fs::create_dir_all(inner.join("assets/minecraft")).unwrap();
    fs::write(inner.join("pack.mcmeta"), common::MCMETA).unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].kind, NodeKind::Shell);
    assert!(report.entries[0]
        .causes
        .iter()
        .any(|cause| cause == "folder_shell"));
    assert_eq!(report.entries[1].kind, NodeKind::Pack);
    assert_eq!(report.entries[1].category, Category::Bloated);
    assert_eq!(report.entries[1].parent_path.as_deref(), Some("Wrapper"));
}

#[test]
fn a_zip_containing_a_pack_zip_is_an_archive_shell_with_a_normal_pack() {
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

    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].kind, NodeKind::Shell);
    assert_eq!(report.entries[1].category, Category::Normal);
    assert_eq!(report.entries[1].parent_path.as_deref(), Some("outer.zip"));
}

#[test]
fn a_compressed_shell_previews_each_direct_inner_pack_independently() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let mut bloated = core_entries();
    bloated.push(("credits.txt", b"extra"));
    make_zip(&inner.path().join("B.zip"), &bloated);
    make_zip(
        &inner.path().join("C.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let a = fs::read(inner.path().join("A.zip")).unwrap();
    let b = fs::read(inner.path().join("B.zip")).unwrap();
    let c = fs::read(inner.path().join("C.zip")).unwrap();
    make_zip(
        &tmp.path().join("Parent.zip"),
        &[
            ("A.zip", a.as_slice()),
            ("B.zip", b.as_slice()),
            ("C.zip", c.as_slice()),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 3);
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.normal, 1);
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.ignored, 1);
    assert_eq!(report.entries.len(), 4);
    let shell = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Parent.zip")
        .unwrap();
    assert_eq!(shell.kind, NodeKind::Shell);
    assert!(shell.causes.iter().any(|cause| cause == "archive_shell"));
    for (name, category) in [
        ("A.zip", Category::Normal),
        ("B.zip", Category::Bloated),
        ("C.zip", Category::Ignored),
    ] {
        let child = report
            .entries
            .iter()
            .find(|entry| entry.relative_path == format!("Parent.zip/{name}"))
            .unwrap();
        assert_eq!(child.parent_path.as_deref(), Some("Parent.zip"));
        assert_eq!(child.category, category);
    }
}

#[test]
fn folder_packs_inside_an_archive_shell_are_classified_by_their_own_layout() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("Folders.zip"),
        &[
            ("Legacy/pack.mcmeta", common::MCMETA),
            (
                "Legacy/assets/minecraft/textures/items/apple.png",
                b"legacy",
            ),
            ("Modern/pack.mcmeta", common::MCMETA),
            ("Modern/assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 2);
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.ignored, 1);
    let legacy = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Folders.zip/Legacy")
        .unwrap();
    assert_eq!(legacy.category, Category::Bloated);
    assert!(legacy.causes.iter().any(|cause| cause == "folder_pack"));
    let modern = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Folders.zip/Modern")
        .unwrap();
    assert_eq!(modern.category, Category::Ignored);
}

#[test]
fn a_classification_folder_inside_an_archive_shell_keeps_its_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    make_zip(&inner.path().join("B.zip"), &core_entries());
    let a = fs::read(inner.path().join("A.zip")).unwrap();
    let b = fs::read(inner.path().join("B.zip")).unwrap();
    make_zip(
        &tmp.path().join("Download.zip"),
        &[
            ("PotPvP/A.zip", a.as_slice()),
            ("PotPvP/B.zip", b.as_slice()),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 2);
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.folder, 1);
    assert_eq!(report.counts.normal, 2);
    let shell = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Download.zip")
        .unwrap();
    assert_eq!(shell.kind, NodeKind::Shell);
    let category = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "Download.zip/PotPvP")
        .unwrap();
    assert_eq!(category.kind, NodeKind::ClassificationFolder);
    assert_eq!(category.parent_path.as_deref(), Some("Download.zip"));
    for name in ["A.zip", "B.zip"] {
        let child = report
            .entries
            .iter()
            .find(|entry| entry.relative_path == format!("Download.zip/PotPvP/{name}"))
            .unwrap();
        assert_eq!(child.parent_path.as_deref(), Some("Download.zip/PotPvP"));
    }
}

#[test]
fn nested_classification_folders_inside_an_archive_keep_every_level() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();
    for name in ["A.zip", "B.zip", "C.zip", "D.zip"] {
        make_zip(&inner.path().join(name), &core_entries());
    }
    let a = fs::read(inner.path().join("A.zip")).unwrap();
    let b = fs::read(inner.path().join("B.zip")).unwrap();
    let c = fs::read(inner.path().join("C.zip")).unwrap();
    let d = fs::read(inner.path().join("D.zip")).unwrap();
    make_zip(
        &tmp.path().join("Download.zip"),
        &[
            ("Collections/PotPvP/A.zip", a.as_slice()),
            ("Collections/PotPvP/B.zip", b.as_slice()),
            ("Collections/Bedwars/C.zip", c.as_slice()),
            ("Collections/Bedwars/D.zip", d.as_slice()),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.total_packs, 4);
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.folder, 3);
    assert_eq!(report.counts.normal, 4);
    for (path, parent) in [
        ("Download.zip/Collections", "Download.zip"),
        (
            "Download.zip/Collections/Bedwars",
            "Download.zip/Collections",
        ),
        (
            "Download.zip/Collections/PotPvP",
            "Download.zip/Collections",
        ),
    ] {
        let folder = report
            .entries
            .iter()
            .find(|entry| entry.relative_path == path)
            .unwrap();
        assert_eq!(folder.kind, NodeKind::ClassificationFolder);
        assert_eq!(folder.parent_path.as_deref(), Some(parent));
    }
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
        &[
            (mcmeta_path.as_str(), common::MCMETA),
            (asset_path.as_str(), b"x"),
        ],
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

    let zip_entry = report
        .entries
        .iter()
        .find(|e| e.name == "pack.zip")
        .unwrap();
    let dir_entry = report
        .entries
        .iter()
        .find(|e| e.name == "FolderPack")
        .unwrap();
    let zip_disk = fs::metadata(tmp.path().join("pack.zip")).unwrap().len();
    assert_eq!(zip_entry.size_bytes, zip_disk);
    assert_eq!(dir_entry.size_bytes, 1000 + common::MCMETA.len() as u64);
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

#[test]
fn a_root_pack_with_singular_item_textures_is_ignored_as_high_version() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("meezoid.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"png"),
        ],
    );

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Ignored);
    assert_eq!(report.counts.ignored, 1);
    let reason = entry.ignore.as_ref().expect("ignore reason");
    assert_eq!(reason.key, "modern_texture_layout");
    assert_eq!(reason.values, vec!["assets/minecraft/textures/item"]);
}

#[test]
fn pack_format_and_mcmeta_parseability_do_not_define_version() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("broken-meta.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":1,"description":"broken",}}"#,
            ),
            ("assets/minecraft/a.png", b"x"),
        ],
    );
    make_zip(
        &tmp.path().join("declares-modern.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":16,"description":"declared modern"}}"#,
            ),
            ("assets/minecraft/textures/items/apple.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.counts.normal, 2);
    assert_eq!(report.counts.ignored, 0);
    assert!(report.entries.iter().all(|entry| entry.ignore.is_none()));
}

#[test]
fn a_root_pack_with_singular_block_textures_is_ignored_as_high_version() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("xray.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/block/", b""),
        ],
    );

    let report = scan(tmp.path());
    let reason = report.entries[0].ignore.as_ref().unwrap();

    assert_eq!(report.entries[0].category, Category::Ignored);
    assert_eq!(reason.key, "modern_texture_layout");
    assert_eq!(reason.values, vec!["assets/minecraft/textures/block"]);
}

#[test]
fn legacy_plural_texture_files_override_singular_paths_in_the_same_core() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("hybrid.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"new"),
            ("assets/minecraft/textures/blocks/", b""),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Normal);
    assert!(report.entries[0].ignore.is_none());
}

#[test]
fn an_empty_singular_item_directory_is_not_version_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("empty-modern-dirs.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/item/", b""),
            ("assets/minecraft/gui/widgets.png", b"png"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Normal);
    assert!(report.entries[0].ignore.is_none());
}

#[test]
fn invalid_lunar_escape_is_still_classified_independently_of_pack_format() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("lunar.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":99,"description":"hi\!"}}"#,
            ),
            ("assets/minecraft/a.png", b"x"),
        ],
    );

    let report = scan(tmp.path());
    let entry = &report.entries[0];

    assert_eq!(entry.category, Category::LunarIllegal);
    assert!(entry.causes.iter().any(|cause| cause == "lunar_escape"));
    assert!(entry.ignore.is_none());
}

#[test]
fn a_nested_high_version_pack_is_ignored_before_unwrap() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();
    make_zip(
        &inner.path().join("Modern.zip"),
        &[
            ("pack.mcmeta", common::MCMETA),
            ("assets/minecraft/textures/block/stone.png", b"x"),
        ],
    );
    make_zip(
        &tmp.path().join("outer.zip"),
        &[(
            "Modern.zip",
            &std::fs::read(inner.path().join("Modern.zip")).unwrap(),
        )],
    );

    let report = scan(tmp.path());
    let modern = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "outer.zip/Modern.zip")
        .unwrap();
    let reason = modern.ignore.as_ref().unwrap();

    assert_eq!(report.entries[0].kind, NodeKind::Shell);
    assert_eq!(modern.category, Category::Ignored);
    assert_eq!(reason.key, "modern_texture_layout");
    assert_eq!(reason.values, vec!["assets/minecraft/textures/block"]);
}

#[test]
fn a_nested_low_version_pack_stays_rescuable() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("outer.zip"),
        &[
            ("Legacy/pack.mcmeta", common::MCMETA),
            ("Legacy/assets/minecraft/textures/items/apple.png", b"x"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Nested);
    assert!(report.entries[0].ignore.is_none());
}

#[test]
fn a_collection_shell_ignores_only_its_high_version_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    make_zip(
        &tmp.path().join("collection.zip"),
        &[
            ("Legacy/pack.mcmeta", common::MCMETA),
            ("Legacy/assets/minecraft/textures/items/apple.png", b"old"),
            ("Modern/pack.mcmeta", common::MCMETA),
            ("Modern/assets/minecraft/textures/item/apple.png", b"new"),
        ],
    );

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].kind, NodeKind::Shell);
    let legacy = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "collection.zip/Legacy")
        .unwrap();
    let modern = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "collection.zip/Modern")
        .unwrap();
    assert_eq!(legacy.category, Category::Bloated);
    assert_eq!(modern.category, Category::Ignored);
    assert_eq!(
        modern.ignore.as_ref().unwrap().values,
        vec!["assets/minecraft/textures/item"]
    );
}

#[test]
fn a_root_folder_with_singular_textures_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("Modern Folder");
    fs::create_dir_all(pack.join("assets/minecraft/textures/item")).unwrap();
    fs::write(pack.join("pack.mcmeta"), common::MCMETA).unwrap();
    fs::write(
        pack.join("assets/minecraft/textures/item/apple.png"),
        b"png",
    )
    .unwrap();

    let report = scan(tmp.path());

    assert_eq!(report.entries[0].category, Category::Ignored);
    assert_eq!(report.counts.ignored, 1);
}

#[test]
fn a_folder_case_variant_with_lunar_escape_keeps_both_rescue_signals() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("Folder");
    std::fs::create_dir_all(pack.join("assets/minecraft")).unwrap();
    std::fs::write(
        pack.join("Pack.mcmeta"),
        br#"{"pack":{"pack_format":1,"description":"x\!"}}"#,
    )
    .unwrap();
    std::fs::write(pack.join("assets/minecraft/a.png"), b"x").unwrap();

    let report = scan(tmp.path());

    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Nested);
    assert!(entry.causes.iter().any(|cause| cause == "mcmeta_rescue"));
    assert!(entry.causes.iter().any(|cause| cause == "lunar_escape"));
}
