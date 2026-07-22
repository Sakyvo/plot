mod common;

use common::{core_entries, make_zip, MCMETA};
use engine::{process, scan, ProcessOptions};
use std::fs;

fn opts_for(tmp: &tempfile::TempDir) -> ProcessOptions {
    ProcessOptions {
        resourcepacks: tmp.path().join("rp"),
        plot_temp: tmp.path().join("plot_temp"),
    }
}

fn setup(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let rp = tmp.path().join("rp");
    fs::create_dir_all(&rp).unwrap();
    rp
}

fn inner_zip_bytes() -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("i.zip");
    make_zip(&p, &core_entries());
    fs::read(&p).unwrap()
}

#[test]
fn zip_wrapping_a_folder_unwraps_to_the_inner_name() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    make_zip(
        &rp.join("download (3).zip"),
        &[
            ("Cool Pack v2/pack.mcmeta", MCMETA),
            ("Cool Pack v2/assets/minecraft/a.png", b"x"),
        ],
    );

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("Cool Pack v2.zip").exists(), "product named after inner layer");
    assert!(!rp.join("download (3).zip").exists());
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.action, "converted");
    assert_eq!(outcome.products, vec!["Cool Pack v2.zip"]);
    assert_eq!(scan(&rp).counts.normal, 1);
}

#[test]
fn folder_wrapping_a_zip_unwraps_too() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    fs::write(wrapper.join("Real Pack.zip"), inner_zip_bytes()).unwrap();

    process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("Real Pack.zip").exists());
    assert_eq!(scan(&rp).counts.normal, 1);
    assert!(tmp.path().join("plot_temp/problematic_packs/Wrapper").exists());
}

#[test]
fn zip_inside_zip_inside_folder_unwraps_across_three_layers() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    // outer.zip -> middle folder -> inner.zip(core)
    let inner = inner_zip_bytes();
    make_zip(
        &rp.join("outer.zip"),
        &[("middle/§bInner Pack.zip", inner.as_slice())],
    );

    process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("§bInner Pack.zip").exists(), "deepest layer name wins");
    assert_eq!(scan(&rp).counts.normal, 1);
}

#[test]
fn a_collection_zip_splits_into_independent_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    make_zip(
        &rp.join("5神包合集.zip"),
        &[
            ("PackA/pack.mcmeta", MCMETA),
            ("PackA/assets/minecraft/a.png", b"a"),
            ("PackB/pack.mcmeta", MCMETA),
            ("PackB/assets/minecraft/b.png", b"b"),
            ("PackC/pack.mcmeta", MCMETA),
            ("PackC/assets/minecraft/c.png", b"c"),
        ],
    );

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("PackA.zip").exists());
    assert!(rp.join("PackB.zip").exists());
    assert!(rp.join("PackC.zip").exists());
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.products.len(), 3);
    assert_eq!(scan(&rp).counts.normal, 3);
}

#[test]
fn two_outer_zips_with_the_same_inner_name_both_survive() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    for outer in ["SoupSkidz Map 7 Pack (1).zip", "§aSoupSkidz Map 7 Pack.zip"] {
        make_zip(
            &rp.join(outer),
            &[
                ("§aSoupSkidz Map 7 Pack/pack.mcmeta", MCMETA),
                ("§aSoupSkidz Map 7 Pack/assets/minecraft/a.png", b"x"),
            ],
        );
    }

    process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("§aSoupSkidz Map 7 Pack.zip").exists());
    assert!(rp.join("§aSoupSkidz Map 7 Pack (1).zip").exists());
    assert_eq!(scan(&rp).counts.normal, 2);
}

#[test]
fn gbk_named_inner_layers_are_decoded_not_mojibake() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    let (name1, _, _) = encoding_rs::GBK.encode("测试包/pack.mcmeta");
    let (name2, _, _) = encoding_rs::GBK.encode("测试包/assets/minecraft/a.png");
    common::make_raw_zip(
        &rp.join("chinese.zip"),
        &[
            (name1.into_owned(), MCMETA),
            (name2.into_owned(), b"x"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    assert!(
        rp.join("测试包.zip").exists(),
        "GBK inner name decoded to real characters"
    );
    assert_eq!(scan(&rp).counts.normal, 1);
}

#[test]
fn nested_pack_missing_mcmeta_gets_unwrapped_and_generated() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    make_zip(
        &rp.join("Infera.zip"),
        &[("§8! §aInfera/assets/minecraft/font/a.png", b"x")],
    );

    process(&opts_for(&tmp)).unwrap();

    let product = rp.join("§8! §aInfera.zip");
    assert!(product.exists());
    let f = fs::File::open(&product).unwrap();
    let mut a = zip::ZipArchive::new(f).unwrap();
    assert!(a.by_name("pack.mcmeta").is_ok(), "mcmeta generated inside unwrapped layer");
    assert_eq!(scan(&rp).counts.normal, 1);
}

#[test]
fn inner_names_with_windows_illegal_characters_are_sanitized() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    make_zip(
        &rp.join("weird.zip"),
        &[
            ("What?: A \"Pack\"/pack.mcmeta", MCMETA),
            ("What?: A \"Pack\"/assets/minecraft/a.png", b"x"),
        ],
    );

    let report = process(&opts_for(&tmp)).unwrap();

    let outcome = &report.outcomes[0];
    assert_eq!(outcome.action, "converted");
    assert_eq!(outcome.products.len(), 1);
    let product = &outcome.products[0];
    assert!(!product.contains('?') && !product.contains(':') && !product.contains('"'));
    assert!(rp.join(product).exists());
    assert_eq!(scan(&rp).counts.normal, 1);
}

#[test]
fn root_fixes_apply_inside_unwrapped_layers() {
    let tmp = tempfile::tempdir().unwrap();
    let rp = setup(&tmp);
    make_zip(
        &rp.join("messy.zip"),
        &[
            ("Inner/pack.mcmeta", MCMETA),
            ("Inner/assets/minecraft/a.png", b"x"),
            ("Inner/credits.txt", b"extra"),
            ("Inner/Thumbs.db", b"junk"),
            ("Inner/assets/minecraft/records/13.ogg", b"dead"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    let product = rp.join("Inner.zip");
    let f = fs::File::open(&product).unwrap();
    let a = zip::ZipArchive::new(f).unwrap();
    let names: Vec<&str> = a.file_names().collect();
    assert!(!names.iter().any(|n| n.contains("credits")));
    assert!(!names.iter().any(|n| n.contains("Thumbs")));
    assert!(!names.iter().any(|n| n.contains("records")));
    assert_eq!(scan(&rp).counts.normal, 1);
}
