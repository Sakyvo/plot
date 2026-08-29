mod common;

use common::{make_zip, MCMETA};
use engine::{process, scan, Category, ProcessOptions};
use std::fs;
use std::io::Read;

/// OTB template: `\!` is the illegal JSON escape Lunar rejects.
const SICK: &[u8] = br#"{"pack":{"pack_format":1,"description":"\u00A7b\! made by the goose :>"}}"#;

/// Color kept, illegal backslash removed — healthy for Lunar.
const PATCHED: &[u8] =
    br#"{"pack":{"pack_format":1,"description":"\u00A7b! made by the goose :>"}}"#;

fn pack_with_mcmeta(dir: &std::path::Path, name: &str, mcmeta: &[u8]) {
    make_zip(
        &dir.join(name),
        &[
            ("pack.mcmeta", mcmeta),
            ("assets/minecraft/textures/x.png", b"png".as_slice()),
        ],
    );
}

fn repair_opts(tmp: &tempfile::TempDir) -> ProcessOptions {
    fs::create_dir_all(tmp.path().join("rp")).unwrap();
    ProcessOptions {
        resourcepacks: tmp.path().join("rp"),
        plot_temp: tmp.path().join("plot_temp"),
        run_dir_name: "Plot_2026-08-23_13.46.34".into(),
    }
}

fn read_zip_mcmeta(zip_path: &std::path::Path) -> Vec<u8> {
    let file = fs::File::open(zip_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("pack.mcmeta").unwrap();
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap();
    bytes
}

fn has_lunar(causes: &[String]) -> bool {
    causes.iter().any(|c| c == "lunar_escape")
}

#[test]
fn pure_lunar_zip_is_tagged_and_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    pack_with_mcmeta(&rp, "OTB FPS.zip", SICK);

    let report = scan(&rp);
    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::LunarIllegal);
    assert!(has_lunar(&entry.causes));
    assert_eq!(report.counts.lunar, 1);
    assert_eq!(report.counts.normal, 0);
}

#[test]
fn healthy_color_escape_is_not_lunar() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    pack_with_mcmeta(&rp, "ok.zip", PATCHED);

    let report = scan(&rp);
    assert_eq!(report.entries[0].category, Category::Normal);
    assert!(!has_lunar(&report.entries[0].causes));
    assert_eq!(report.counts.lunar, 0);
    assert_eq!(report.counts.normal, 1);
}

#[test]
fn section_literal_and_bom_do_not_alone_mark_lunar() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    // literal § (U+00A7 as UTF-8 C2 A7), no invalid escape
    let with_section: &[u8] = b"{\"pack\":{\"pack_format\":1,\"description\":\"\xc2\xa7b hello\"}}";
    let with_bom: &[u8] = b"\xef\xbb\xbf{\"pack\":{\"pack_format\":1,\"description\":\"hi\"}}";
    pack_with_mcmeta(&rp, "sec.zip", with_section);
    pack_with_mcmeta(&rp, "bom.zip", with_bom);

    let report = scan(&rp);
    for e in &report.entries {
        assert_eq!(e.category, Category::Normal, "{}", e.name);
        assert!(!has_lunar(&e.causes), "{}", e.name);
    }
    assert_eq!(report.counts.lunar, 0);
}

#[test]
fn nested_plus_lunar_keeps_structure_and_stacks_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    let entries: Vec<(&str, &[u8])> = vec![
        ("Inner Pack/pack.mcmeta", SICK),
        ("Inner Pack/assets/minecraft/x.png", b"png"),
    ];
    make_zip(&rp.join("wrapped.zip"), &entries);

    let report = scan(&rp);
    let shell = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "wrapped.zip")
        .unwrap();
    assert_eq!(shell.category, Category::Nested);
    assert!(!has_lunar(&shell.causes));
    let inner = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == "wrapped.zip/Inner Pack")
        .unwrap();
    assert_eq!(inner.category, Category::Bloated);
    assert!(has_lunar(&inner.causes));
    assert_eq!(report.counts.nested, 1);
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.lunar, 1);
    // multi-tag: sum of counts can exceed pack count
    assert_eq!(report.total_packs, 1);
}

#[test]
fn bloated_plus_lunar_stacks() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    make_zip(
        &rp.join("fat.zip"),
        &[
            ("pack.mcmeta", SICK),
            ("assets/minecraft/x.png", b"png".as_slice()),
            ("readme.txt", b"extra".as_slice()),
        ],
    );

    let report = scan(&rp);
    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Bloated);
    assert!(has_lunar(&entry.causes));
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.lunar, 1);
}

#[test]
fn folder_pack_with_lunar_is_bloated_plus_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    let pack = rp.join("folderpack");
    fs::create_dir_all(pack.join("assets/minecraft")).unwrap();
    fs::write(pack.join("pack.mcmeta"), SICK).unwrap();
    fs::write(pack.join("assets/minecraft/x.png"), b"png").unwrap();

    let report = scan(&rp);
    let entry = &report.entries[0];
    assert_eq!(entry.category, Category::Bloated);
    assert!(entry.causes.iter().any(|cause| cause == "folder_pack"));
    assert!(has_lunar(&entry.causes));
    assert_eq!(report.counts.bloated, 1);
    assert_eq!(report.counts.folder, 0);
    assert_eq!(report.counts.lunar, 1);
}

#[test]
fn illegal_pack_never_gets_lunar_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    fs::write(rp.join("note.txt"), b"not a zip").unwrap();

    let report = scan(&rp);
    assert_eq!(report.entries[0].category, Category::Illegal);
    assert!(!has_lunar(&report.entries[0].causes));
    assert_eq!(report.counts.lunar, 0);
}

#[test]
fn nested_product_mcmeta_is_patched() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = repair_opts(&tmp);
    let entries: Vec<(&str, &[u8])> = vec![
        ("Inner Pack/pack.mcmeta", SICK),
        ("Inner Pack/assets/minecraft/x.png", b"png"),
    ];
    make_zip(&opts.resourcepacks.join("wrapped.zip"), &entries);

    process(&opts).unwrap();

    let product = opts.resourcepacks.join("Inner Pack.zip");
    assert!(product.exists());
    let mcmeta = read_zip_mcmeta(&product);
    assert!(serde_json::from_slice::<serde_json::Value>(&mcmeta).is_ok());
    assert!(String::from_utf8(mcmeta)
        .unwrap()
        .contains(r"\u00A7b! made"));
}

#[test]
fn hopeless_mcmeta_in_nested_product_is_regenerated() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = repair_opts(&tmp);
    let dead: &[u8] = br#"{"pack": "description": "\! x""#;
    let entries: Vec<(&str, &[u8])> = vec![
        ("Inner/pack.mcmeta", dead),
        ("Inner/assets/minecraft/x.png", b"png"),
    ];
    make_zip(&opts.resourcepacks.join("wrapped.zip"), &entries);

    process(&opts).unwrap();

    let mcmeta = read_zip_mcmeta(&opts.resourcepacks.join("Inner.zip"));
    let value: serde_json::Value = serde_json::from_slice(&mcmeta).unwrap();
    assert_eq!(value["pack"]["pack_format"], 1);
    assert_eq!(value["pack"]["description"], "Inner");
}

#[test]
fn healthy_normal_pack_is_untouched_by_processing() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = repair_opts(&tmp);
    pack_with_mcmeta(&opts.resourcepacks, "clean.zip", MCMETA);
    let before = fs::read(opts.resourcepacks.join("clean.zip")).unwrap();

    let report = process(&opts).unwrap();

    assert!(report.outcomes.is_empty());
    assert_eq!(
        fs::read(opts.resourcepacks.join("clean.zip")).unwrap(),
        before
    );
}

#[test]
fn pure_lunar_pack_is_converted_with_color_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = repair_opts(&tmp);
    pack_with_mcmeta(&opts.resourcepacks, "OTB FPS.zip", SICK);

    let report = process(&opts).unwrap();

    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].action, "converted");
    assert!(opts
        .plot_temp
        .join("Plot_2026-08-23_13.46.34")
        .join("problematic_packs")
        .join("OTB FPS.zip")
        .exists());
    let products = &report.outcomes[0].products;
    assert!(!products.is_empty());
    let product = opts.resourcepacks.join(&products[0]);
    assert!(product.exists());
    let mcmeta = read_zip_mcmeta(&product);
    let text = String::from_utf8(mcmeta.clone()).unwrap();
    assert!(
        text.contains(r"\u00A7b! made"),
        "color kept, escape fixed: {text}"
    );
    assert!(!text.contains(r"\!"), "illegal escape removed: {text}");
    assert!(serde_json::from_slice::<serde_json::Value>(&mcmeta).is_ok());
}

#[test]
fn nested_plus_lunar_is_unwrapped_and_patched_once() {
    let tmp = tempfile::tempdir().unwrap();
    let opts = repair_opts(&tmp);
    let entries: Vec<(&str, &[u8])> = vec![
        ("Inner Pack/pack.mcmeta", SICK),
        ("Inner Pack/assets/minecraft/x.png", b"png"),
    ];
    make_zip(&opts.resourcepacks.join("wrapped.zip"), &entries);

    let report = process(&opts).unwrap();

    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].action, "converted");
    let product = opts.resourcepacks.join("Inner Pack.zip");
    assert!(product.exists());
    let mcmeta = read_zip_mcmeta(&product);
    let text = String::from_utf8(mcmeta).unwrap();
    assert!(text.contains(r"\u00A7b! made"));
    assert!(!text.contains(r"\!"));
}
