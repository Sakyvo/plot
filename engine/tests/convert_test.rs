mod common;

use common::{core_entries, make_zip, MCMETA};
use engine::{process, scan, Category, ProcessOptions};
use std::fs;
use std::io::Read;

fn opts_for(tmp: &tempfile::TempDir) -> ProcessOptions {
    ProcessOptions {
        resourcepacks: tmp.path().join("rp"),
        plot_temp: tmp.path().join("plot_temp"),
    }
}

fn setup(tmp: &tempfile::TempDir) {
    fs::create_dir_all(tmp.path().join("rp")).unwrap();
}

fn zip_entry_bytes(zip_path: &std::path::Path, entry: &str) -> Option<Vec<u8>> {
    let f = fs::File::open(zip_path).ok()?;
    let mut a = zip::ZipArchive::new(f).ok()?;
    let mut e = a.by_name(entry).ok()?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn zip_names(zip_path: &std::path::Path) -> Vec<String> {
    let f = fs::File::open(zip_path).unwrap();
    let a = zip::ZipArchive::new(f).unwrap();
    a.file_names().map(|n| n.to_string()).collect()
}

#[test]
fn a_folder_pack_is_converted_to_a_normal_zip() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let pack = rp.join("MyPack");
    fs::create_dir_all(pack.join("assets/minecraft/textures")).unwrap();
    fs::write(pack.join("pack.mcmeta"), MCMETA).unwrap();
    fs::write(pack.join("assets/minecraft/textures/a.png"), b"pngdata").unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    let product = rp.join("MyPack.zip");
    assert!(product.exists(), "product zip created");
    assert!(!pack.exists(), "folder moved out of rp");
    assert!(tmp.path().join("plot_temp/problematic_packs/MyPack").exists());
    assert_eq!(
        zip_entry_bytes(&product, "assets/minecraft/textures/a.png").unwrap(),
        b"pngdata"
    );
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.action, "converted");
    assert_eq!(outcome.products, vec!["MyPack.zip"]);
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn bloated_packs_are_slimmed_to_the_three_core_files() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let mut entries = core_entries();
    entries.push(("assets/minecraft/records/13.ogg", b"deadmusic" as &[u8]));
    entries.push(("credits.txt", b"by someone"));
    entries.push(("Thumbs.db", b"junk"));
    entries.push(("assets/minecraft/.DS_Store", b"junk"));
    make_zip(&rp.join("Yokabi.zip"), &entries);

    process(&opts_for(&tmp)).unwrap();

    let product = rp.join("Yokabi.zip");
    assert!(product.exists());
    let names = zip_names(&product);
    assert!(!names.iter().any(|n| n.contains("records")), "dead path gone");
    assert!(!names.iter().any(|n| n.contains("credits")), "extras gone");
    assert!(!names.iter().any(|n| n.contains("Thumbs") || n.contains(".DS_Store")));
    assert_eq!(
        zip_entry_bytes(&product, "assets/minecraft/textures/blocks/stone.png").unwrap(),
        b"png",
        "legit content byte-identical"
    );
    assert!(tmp.path().join("plot_temp/problematic_packs/Yokabi.zip").exists());
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn typo_core_files_are_renamed_preserving_author_content() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("Kiro.zip"),
        &[
            ("pack.mcmeta.mcmeta", MCMETA),
            ("pack..png", b"authoricon" as &[u8]),
            ("assets/minecraft/a.png", b"x"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    let product = rp.join("Kiro.zip");
    assert_eq!(zip_entry_bytes(&product, "pack.mcmeta").unwrap(), MCMETA);
    assert_eq!(zip_entry_bytes(&product, "pack.png").unwrap(), b"authoricon");
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn a_missing_mcmeta_is_generated_with_the_pack_name_as_description() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("Infera.zip"),
        &[("assets/minecraft/font/a.png", b"x" as &[u8])],
    );

    process(&opts_for(&tmp)).unwrap();

    let generated = zip_entry_bytes(&rp.join("Infera.zip"), "pack.mcmeta").unwrap();
    let text = String::from_utf8(generated).unwrap();
    assert!(text.contains("\"pack_format\""));
    assert!(text.contains("Infera"));
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn wrong_extensions_are_corrected_to_lowercase_zip() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(&rp.join("UPPER.ZIP"), &core_entries());
    make_zip(&rp.join("fake.rar"), &core_entries());

    process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("UPPER.zip").exists());
    assert!(rp.join("fake.zip").exists());
    let rescan = scan(&rp);
    assert_eq!(rescan.counts.normal, 2);
}

#[test]
fn product_name_collisions_get_a_suffix_and_never_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(&rp.join("Pack.zip"), &core_entries());
    let pack_dir = rp.join("Pack");
    fs::create_dir_all(pack_dir.join("assets/minecraft")).unwrap();
    fs::write(pack_dir.join("pack.mcmeta"), MCMETA).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    let folder_outcome = report
        .outcomes
        .iter()
        .find(|o| o.original_name == "Pack")
        .unwrap();
    assert_eq!(folder_outcome.products, vec!["Pack (1).zip"]);
    assert!(rp.join("Pack.zip").exists(), "unrelated pack untouched");
    assert!(rp.join("Pack (1).zip").exists());
}

#[test]
fn a_pack_that_fails_mid_conversion_keeps_its_original_in_problematic_packs() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    // Classifiable (names list fine) but the entry data is corrupted, so
    // conversion blows up when streaming the entry out.
    let zip_path = rp.join("Broken.zip");
    let mut entries = core_entries();
    let big = vec![0x41u8; 20000];
    entries.push(("assets/minecraft/big.bin", big.as_slice()));
    entries.push(("credits.txt", b"extra so it is bloated"));
    make_zip(&zip_path, &entries);
    let mut bytes = fs::read(&zip_path).unwrap();
    let marker = b"assets/minecraft/big.bin";
    let pos = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .unwrap();
    for b in &mut bytes[pos + marker.len() + 40..pos + marker.len() + 60] {
        *b ^= 0xff;
    }
    fs::write(&zip_path, bytes).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    let outcome = &report.outcomes[0];
    assert_eq!(outcome.action, "failed");
    assert!(
        tmp.path()
            .join("plot_temp/problematic_packs/Broken.zip")
            .exists(),
        "original preserved for manual recovery"
    );
    assert!(!rp.join("Broken.zip").exists(), "no half-product left in rp");
}

#[cfg(windows)]
#[test]
fn a_locked_pack_is_skipped_and_reported() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let mut entries = core_entries();
    entries.push(("credits.txt", b"extra" as &[u8]));
    make_zip(&rp.join("InUse.zip"), &entries);
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2) // allow read/write sharing, deny delete — how MC holds packs
        .open(rp.join("InUse.zip"))
        .unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    let outcome = &report.outcomes[0];
    assert_eq!(outcome.action, "skipped_locked");
    assert!(rp.join("InUse.zip").exists(), "locked pack left in place");
}
