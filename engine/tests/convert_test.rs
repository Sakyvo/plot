mod common;

use common::{core_entries, make_zip, MCMETA};
use engine::{process, scan, Category, ProcessOptions};
use std::fs;
use std::io::Read;

fn opts_for(tmp: &tempfile::TempDir) -> ProcessOptions {
    ProcessOptions {
        resourcepacks: tmp.path().join("rp"),
        plot_temp: tmp.path().join("plot_temp"),
        run_dir_name: "Plot_2026-08-23_13.46.34".into(),
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
fn classified_packs_process_in_place_and_mirror_quarantine_paths() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let category = rp.join("PotPvP");
    fs::create_dir_all(&category).unwrap();
    make_zip(&category.join("A.zip"), &core_entries());
    let mut bloated = core_entries();
    bloated.push(("credits.txt", b"extra"));
    make_zip(&category.join("B.zip"), &bloated);
    fs::write(category.join("C.zip"), b"broken").unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(category.join("A.zip").exists());
    assert!(category.join("B.zip").exists());
    assert!(!category.join("C.zip").exists());
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/PotPvP/B.zip")
        .exists());
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/illegal_packs/PotPvP/C.zip")
        .exists());
    let repaired = report
        .outcomes
        .iter()
        .find(|outcome| outcome.original_name == "PotPvP/B.zip")
        .unwrap();
    assert_eq!(repaired.products, vec!["PotPvP/B.zip"]);
    assert!(report
        .outcomes
        .iter()
        .any(|outcome| outcome.original_name == "PotPvP/C.zip"));
}

#[test]
fn classified_targets_ignore_legacy_flat_leftovers_and_keep_plain_mirror_names() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let category = rp.join("PotPvP");
    let folder_pack = category.join("Pack");
    fs::create_dir_all(folder_pack.join("assets/minecraft")).unwrap();
    fs::write(folder_pack.join("pack.mcmeta"), MCMETA).unwrap();
    make_zip(&category.join("Pack.zip"), &core_entries());
    fs::write(category.join("Bad.zip"), b"bad").unwrap();
    // Leftovers from the legacy flat layout stay untouched and never leak
    // suffixes into the fresh run folder.
    fs::create_dir_all(tmp.path().join("plot_temp/problematic_packs/PotPvP/Pack")).unwrap();
    fs::write(
        tmp.path()
            .join("plot_temp/problematic_packs/PotPvP/Pack/keep.txt"),
        b"keep",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("plot_temp/illegal_packs/PotPvP")).unwrap();
    fs::write(
        tmp.path().join("plot_temp/illegal_packs/PotPvP/Bad.zip"),
        b"old",
    )
    .unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(
        category.join("Pack.zip").exists(),
        "existing normal pack kept"
    );
    assert!(
        category.join("Pack (1).zip").exists(),
        "folder product deconflicted"
    );
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/PotPvP/Pack")
        .exists());
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/illegal_packs/PotPvP/Bad.zip")).unwrap(),
        b"bad"
    );
    // Legacy flat leftovers preserved byte-for-byte.
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/illegal_packs/PotPvP/Bad.zip")).unwrap(),
        b"old"
    );
    let outcome = report
        .outcomes
        .iter()
        .find(|outcome| outcome.original_name == "PotPvP/Pack")
        .unwrap();
    assert_eq!(outcome.products, vec!["PotPvP/Pack (1).zip"]);
}

#[test]
fn a_failed_classified_sibling_is_restored_without_rolling_back_successes() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let category = rp.join("PotPvP");
    fs::create_dir_all(&category).unwrap();
    let mut good = core_entries();
    good.push(("credits.txt", b"extra"));
    make_zip(&category.join("Good.zip"), &good);

    let broken_path = category.join("Broken.zip");
    let big = vec![0x41u8; 20_000];
    let mut broken = core_entries();
    broken.push(("assets/minecraft/big.bin", big.as_slice()));
    broken.push(("credits.txt", b"extra"));
    make_zip(&broken_path, &broken);
    let mut original = fs::read(&broken_path).unwrap();
    let marker = b"assets/minecraft/big.bin";
    let pos = original
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    for byte in &mut original[pos + marker.len() + 40..pos + marker.len() + 60] {
        *byte ^= 0xff;
    }
    fs::write(&broken_path, &original).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(category.join("Good.zip").exists());
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/PotPvP/Good.zip")
        .exists());
    assert_eq!(fs::read(&broken_path).unwrap(), original);
    assert!(!tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/PotPvP/Broken.zip")
        .exists());
    assert_eq!(
        report
            .outcomes
            .iter()
            .find(|outcome| outcome.original_name == "PotPvP/Broken.zip")
            .unwrap()
            .action,
        "failed"
    );
}

#[test]
fn a_normal_pack_is_moved_byte_for_byte_out_of_a_single_folder_shell() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    make_zip(&wrapper.join("A.zip"), &core_entries());
    let original = fs::read(wrapper.join("A.zip")).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!wrapper.exists());
    assert_eq!(fs::read(rp.join("A.zip")).unwrap(), original);
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].original_name, "Wrapper");
    assert_eq!(report.outcomes[0].action, "converted");
    assert_eq!(report.outcomes[0].products, vec!["A.zip"]);
}

#[test]
fn a_problem_pack_is_repaired_while_its_folder_shell_is_backed_up() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    let mut entries = core_entries();
    entries.push(("credits.txt", b"extra"));
    make_zip(&wrapper.join("A.zip"), &entries);

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!wrapper.exists());
    assert!(rp.join("A.zip").exists());
    assert!(!zip_names(&rp.join("A.zip"))
        .iter()
        .any(|name| name == "credits.txt"));
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper/A.zip")
        .exists());
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].action, "converted");
    assert_eq!(report.outcomes[0].products, vec!["A.zip"]);
}

#[test]
fn ignored_zip_and_folder_packs_leave_single_folder_shells_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");

    let zip_wrapper = rp.join("ZipWrapper");
    fs::create_dir_all(&zip_wrapper).unwrap();
    make_zip(
        &zip_wrapper.join("Modern.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let zip_bytes = fs::read(zip_wrapper.join("Modern.zip")).unwrap();

    let folder_wrapper = rp.join("FolderWrapper");
    let modern_folder = folder_wrapper.join("ModernFolder");
    fs::create_dir_all(modern_folder.join("assets/minecraft/textures/item")).unwrap();
    fs::write(modern_folder.join("pack.mcmeta"), MCMETA).unwrap();
    fs::write(
        modern_folder.join("assets/minecraft/textures/item/apple.png"),
        b"folder-modern",
    )
    .unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("Modern.zip")).unwrap(), zip_bytes);
    assert_eq!(
        fs::read(rp.join("ModernFolder/assets/minecraft/textures/item/apple.png")).unwrap(),
        b"folder-modern"
    );
    assert!(rp.join("ModernFolder").is_dir());
    assert!(!zip_wrapper.exists());
    assert!(!folder_wrapper.exists());
    assert_eq!(report.outcomes.len(), 2);
    let rescanned = scan(&rp);
    assert_eq!(rescanned.counts.ignored, 2);
}

#[test]
fn an_illegal_pack_inside_a_single_folder_shell_is_quarantined_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    fs::write(wrapper.join("Bad.zip"), b"broken archive").unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!wrapper.exists());
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper/Bad.zip")
        .exists());
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/illegal_packs/Bad.zip")).unwrap(),
        b"broken archive"
    );
    assert_eq!(report.outcomes[0].action, "moved_to_illegal");
    assert_eq!(report.outcomes[0].products, vec!["Bad.zip"]);
}

#[test]
fn a_nested_pack_inside_a_single_folder_shell_uses_the_normal_unwrap_flow() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    make_zip(
        &wrapper.join("Download.zip"),
        &[
            ("Inner/pack.mcmeta", MCMETA),
            ("Inner/assets/minecraft/a.png", b"inner"),
        ],
    );

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!wrapper.exists());
    assert!(rp.join("Inner.zip").exists());
    assert_eq!(
        zip_entry_bytes(&rp.join("Inner.zip"), "assets/minecraft/a.png").unwrap(),
        b"inner"
    );
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper/Download.zip")
        .exists());
    assert_eq!(report.outcomes[0].action, "converted");
    assert_eq!(report.outcomes[0].products, vec!["Inner.zip"]);
}

#[test]
fn attachments_preserve_a_single_pack_folder_while_known_junk_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");

    let kept = rp.join("Download");
    fs::create_dir_all(&kept).unwrap();
    let mut bloated = core_entries();
    bloated.push(("credits.txt", b"extra"));
    make_zip(&kept.join("A.zip"), &bloated);
    fs::write(kept.join("preview.png"), b"preview").unwrap();

    let collapsed = rp.join("Wrapper");
    fs::create_dir_all(&collapsed).unwrap();
    make_zip(&collapsed.join("B.zip"), &core_entries());
    fs::write(collapsed.join("Thumbs.db"), b"junk").unwrap();

    process(&opts_for(&tmp)).unwrap();

    assert!(kept.is_dir());
    assert!(kept.join("A.zip").exists());
    assert_eq!(fs::read(kept.join("preview.png")).unwrap(), b"preview");
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Download/A.zip")
        .exists());
    assert!(!collapsed.exists());
    assert!(rp.join("B.zip").exists());
}

#[cfg(windows)]
#[test]
fn a_locked_child_keeps_its_folder_shell_atomic() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    make_zip(&wrapper.join("A.zip"), &core_entries());
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(wrapper.join("A.zip"))
        .unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(report.outcomes[0].action, "skipped_locked");
    assert!(wrapper.join("A.zip").exists());
    assert!(!rp.join("A.zip").exists());
    assert!(!tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper")
        .exists());
}

#[test]
fn a_shell_prepare_failure_leaves_the_complete_wrapper_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    let path = wrapper.join("Broken.zip");
    let big = vec![0x41u8; 20_000];
    let mut entries = core_entries();
    entries.push(("assets/minecraft/big.bin", big.as_slice()));
    entries.push(("credits.txt", b"extra"));
    make_zip(&path, &entries);
    let mut original = fs::read(&path).unwrap();
    let marker = b"assets/minecraft/big.bin";
    let pos = original
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    for byte in &mut original[pos + marker.len() + 40..pos + marker.len() + 60] {
        *byte ^= 0xff;
    }
    fs::write(&path, &original).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(report.outcomes[0].action, "failed");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!rp.join("Broken.zip").exists());
    assert!(!tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper")
        .exists());
}

#[test]
fn a_same_second_run_folder_never_overwrites_existing_data() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let wrapper = rp.join("Wrapper");
    fs::create_dir_all(&wrapper).unwrap();
    let mut entries = core_entries();
    entries.push(("credits.txt", b"extra"));
    make_zip(&wrapper.join("A.zip"), &entries);
    let existing = tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Wrapper");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("keep.txt"), b"keep").unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    // The run folder name is taken, so the batch lands in \" (1)\" and the
    // occupying data is untouched.
    assert_eq!(report.run_dir.as_deref(), Some("Plot_2026-08-23_13.46.34 (1)"));
    assert_eq!(fs::read(existing.join("keep.txt")).unwrap(), b"keep");
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34 (1)/problematic_packs/Wrapper/A.zip")
        .exists());
    assert!(rp.join("A.zip").exists());
}

#[test]
fn a_compressed_shell_commits_mixed_inner_packs_as_one_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    let mut bloated = core_entries();
    bloated.push(("credits.txt", b"extra"));
    make_zip(&inner.path().join("B.zip"), &bloated);
    make_zip(
        &inner.path().join("C.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let c_bytes = fs::read(inner.path().join("C.zip")).unwrap();
    let b_bytes = fs::read(inner.path().join("B.zip")).unwrap();
    let outer = rp.join("Parent.zip");
    make_zip(
        &outer,
        &[
            ("A.zip", a_bytes.as_slice()),
            ("B.zip", b_bytes.as_slice()),
            ("C.zip", c_bytes.as_slice()),
            ("D.zip", b"broken archive"),
        ],
    );
    let outer_bytes = fs::read(&outer).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!outer.exists());
    assert_eq!(fs::read(rp.join("A.zip")).unwrap(), a_bytes);
    assert!(rp.join("B.zip").exists());
    assert!(!zip_names(&rp.join("B.zip"))
        .iter()
        .any(|name| name == "credits.txt"));
    assert_eq!(fs::read(rp.join("C.zip")).unwrap(), c_bytes);
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/illegal_packs/D.zip")).unwrap(),
        b"broken archive"
    );
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Parent.zip")).unwrap(),
        outer_bytes
    );
    assert_eq!(scan(&rp).counts.ignored, 1);
    assert!(report
        .outcomes
        .iter()
        .any(|outcome| outcome.original_name == "Parent.zip/A.zip"));
    assert!(report
        .outcomes
        .iter()
        .any(|outcome| outcome.original_name == "Parent.zip/D.zip"
            && outcome.action == "moved_to_illegal"));
}

#[test]
fn an_archive_shell_releases_a_classification_folder_with_its_tree() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    make_zip(&inner.path().join("B.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    let b_bytes = fs::read(inner.path().join("B.zip")).unwrap();
    let outer = rp.join("Download.zip");
    make_zip(
        &outer,
        &[
            ("PotPvP/A.zip", a_bytes.as_slice()),
            ("PotPvP/B.zip", b_bytes.as_slice()),
        ],
    );
    let outer_bytes = fs::read(&outer).unwrap();

    process(&opts_for(&tmp)).unwrap();

    assert!(!outer.exists());
    assert_eq!(fs::read(rp.join("PotPvP/A.zip")).unwrap(), a_bytes);
    assert_eq!(fs::read(rp.join("PotPvP/B.zip")).unwrap(), b_bytes);
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Download.zip")).unwrap(),
        outer_bytes
    );
}

#[test]
fn an_archive_shell_preserves_a_single_pack_folder_with_attachments() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    let preview = b"preview bytes";
    make_zip(
        &rp.join("Download.zip"),
        &[
            ("Preview/A.zip", a_bytes.as_slice()),
            ("Preview/preview.png", preview.as_slice()),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("Preview/A.zip")).unwrap(), a_bytes);
    assert_eq!(fs::read(rp.join("Preview/preview.png")).unwrap(), preview);
}

#[test]
fn root_attachments_stay_in_the_original_archive_and_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    make_zip(
        &rp.join("Download.zip"),
        &[
            ("A.zip", a_bytes.as_slice()),
            ("readme.txt", b"download page notes"),
        ],
    );

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("A.zip")).unwrap(), a_bytes);
    assert!(!rp.join("readme.txt").exists());
    assert_eq!(report.notices.len(), 1);
    assert_eq!(
        report.notices[0].key,
        "attachments_kept_in_original_archive"
    );
    assert_eq!(report.notices[0].values, vec!["Download.zip"]);
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Download.zip")
        .exists());
}

#[test]
fn a_redundant_archive_directory_without_attachments_is_collapsed() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    make_zip(
        &rp.join("Download.zip"),
        &[("Wrapper/A.zip", a_bytes.as_slice())],
    );

    process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("A.zip")).unwrap(), a_bytes);
    assert!(!rp.join("Wrapper").exists());
}

#[test]
fn classified_archive_packs_keep_independent_actions_and_mirrored_paths() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();
    let mut bloated = core_entries();
    bloated.push(("credits.txt", b"extra"));
    make_zip(&inner.path().join("B.zip"), &bloated);
    let b_bytes = fs::read(inner.path().join("B.zip")).unwrap();
    make_zip(
        &inner.path().join("C.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let c_bytes = fs::read(inner.path().join("C.zip")).unwrap();
    make_zip(
        &rp.join("Download.zip"),
        &[
            ("PotPvP/A.zip", a_bytes.as_slice()),
            ("PotPvP/B.zip", b_bytes.as_slice()),
            ("PotPvP/C.zip", c_bytes.as_slice()),
            ("PotPvP/D.zip", b"broken archive"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("PotPvP/A.zip")).unwrap(), a_bytes);
    assert_ne!(fs::read(rp.join("PotPvP/B.zip")).unwrap(), b_bytes);
    assert!(!zip_names(&rp.join("PotPvP/B.zip"))
        .iter()
        .any(|name| name == "credits.txt"));
    assert_eq!(fs::read(rp.join("PotPvP/C.zip")).unwrap(), c_bytes);
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/illegal_packs/PotPvP/D.zip")).unwrap(),
        b"broken archive"
    );
    let rescanned = scan(&rp);
    assert_eq!(rescanned.counts.normal, 2);
    assert_eq!(rescanned.counts.ignored, 1);
    assert_eq!(rescanned.counts.illegal, 0);
}

#[test]
fn folder_packs_from_an_archive_shell_preserve_or_normalize_their_shape() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("Folders.zip"),
        &[
            ("Legacy/pack.mcmeta", MCMETA),
            (
                "Legacy/assets/minecraft/textures/items/apple.png",
                b"legacy",
            ),
            ("Modern/pack.mcmeta", MCMETA),
            ("Modern/assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("Legacy.zip").exists());
    assert_eq!(
        zip_entry_bytes(
            &rp.join("Legacy.zip"),
            "assets/minecraft/textures/items/apple.png"
        )
        .unwrap(),
        b"legacy"
    );
    assert!(rp.join("Modern").is_dir());
    assert_eq!(
        fs::read(rp.join("Modern/assets/minecraft/textures/item/apple.png")).unwrap(),
        b"modern"
    );
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Folders.zip")
        .exists());
    let rescanned = scan(&rp);
    assert_eq!(rescanned.counts.normal, 1);
    assert_eq!(rescanned.counts.ignored, 1);
}

#[test]
fn a_compressed_shell_prepare_failure_commits_nothing_and_keeps_the_outer_file() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let a_bytes = fs::read(inner.path().join("A.zip")).unwrap();

    let big = vec![0x41u8; 20_000];
    let mut broken_entries = core_entries();
    broken_entries.push(("assets/minecraft/big.bin", big.as_slice()));
    broken_entries.push(("credits.txt", b"extra"));
    let broken_path = inner.path().join("Broken.zip");
    make_zip(&broken_path, &broken_entries);
    let mut broken = fs::read(&broken_path).unwrap();
    let marker = b"assets/minecraft/big.bin";
    let pos = broken
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    for byte in &mut broken[pos + marker.len() + 40..pos + marker.len() + 60] {
        *byte ^= 0xff;
    }

    let outer = rp.join("Parent.zip");
    make_zip(
        &outer,
        &[
            ("A.zip", a_bytes.as_slice()),
            ("Broken.zip", broken.as_slice()),
        ],
    );
    let original = fs::read(&outer).unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].action, "failed");
    assert_eq!(fs::read(&outer).unwrap(), original);
    assert!(!rp.join("A.zip").exists());
    assert!(!rp.join("Broken.zip").exists());
    assert!(!tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Parent.zip")
        .exists());
}

#[cfg(windows)]
#[test]
fn a_locked_compressed_shell_leaves_no_backup_or_inner_products() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner = tempfile::tempdir().unwrap();
    make_zip(&inner.path().join("A.zip"), &core_entries());
    let bytes = fs::read(inner.path().join("A.zip")).unwrap();
    let outer = rp.join("Parent.zip");
    make_zip(&outer, &[("A.zip", bytes.as_slice())]);
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&outer)
        .unwrap();

    let report = process(&opts_for(&tmp)).unwrap();

    assert_eq!(report.outcomes[0].action, "skipped_locked");
    assert!(outer.exists());
    assert!(!rp.join("A.zip").exists());
    assert!(!tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Parent.zip")
        .exists());
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
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/MyPack")
        .exists());
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
    assert!(
        !names.iter().any(|n| n.contains("records")),
        "dead path gone"
    );
    assert!(!names.iter().any(|n| n.contains("credits")), "extras gone");
    assert!(!names
        .iter()
        .any(|n| n.contains("Thumbs") || n.contains(".DS_Store")));
    assert_eq!(
        zip_entry_bytes(&product, "assets/minecraft/textures/blocks/stone.png").unwrap(),
        b"png",
        "legit content byte-identical"
    );
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Yokabi.zip")
        .exists());
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
    assert_eq!(
        zip_entry_bytes(&product, "pack.png").unwrap(),
        b"authoricon"
    );
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn consistent_mcmeta_candidates_use_stable_source_priority() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let lower_priority = br#"{"pack":{"pack_format":1,"description":"text suffix"}}"#;
    let preferred = br#"{"pack":{"pack_format":1,"description":"case variant"}}"#;
    make_zip(
        &rp.join("Priority.zip"),
        &[
            ("pack.mcmeta.txt", lower_priority),
            ("Pack.mcmeta", preferred),
            ("assets/minecraft/a.png", b"x"),
        ],
    );

    process(&opts_for(&tmp)).unwrap();

    assert_eq!(
        zip_entry_bytes(&rp.join("Priority.zip"), "pack.mcmeta").unwrap(),
        preferred
    );
}

#[test]
fn a_missing_mcmeta_is_generated_for_a_rescuable_pack() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("Infera.zip"),
        &[("assets/minecraft/font/a.png", b"x" as &[u8])],
    );

    process(&opts_for(&tmp)).unwrap();

    assert!(zip_entry_bytes(&rp.join("Infera.zip"), "pack.mcmeta").is_some());
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);
}

#[test]
fn mcmeta_content_does_not_trigger_processing_by_itself() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("No Description.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":1,"author_field":"keep"},"root_field":7}"#,
            ),
            ("assets/minecraft/a.png", b"x"),
        ],
    );

    let before = fs::read(rp.join("No Description.zip")).unwrap();
    assert_eq!(scan(&rp).entries[0].category, Category::Normal);

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(report.outcomes.is_empty());
    assert_eq!(fs::read(rp.join("No Description.zip")).unwrap(), before);
}

#[test]
fn valid_parent_separates_an_embedded_modern_zip_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner_tmp = tempfile::tempdir().unwrap();
    make_zip(
        &inner_tmp.path().join("bonus-modern.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":5,"description":"modern"}}"#,
            ),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let inner_bytes = fs::read(inner_tmp.path().join("bonus-modern.zip")).unwrap();
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("bonus-modern.zip", &inner_bytes),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("Parent.zip").exists());
    assert_eq!(fs::read(rp.join("bonus-modern.zip")).unwrap(), inner_bytes);
    assert_eq!(scan(&rp).counts.ignored, 1);
    let outcome = result
        .outcomes
        .iter()
        .find(|outcome| outcome.original_name == "Parent.zip")
        .unwrap();
    assert_eq!(outcome.separated[0].name, "bonus-modern.zip");
    assert_eq!(outcome.separated[0].parent, "Parent.zip");
}

#[test]
fn valid_parent_separates_an_embedded_low_version_zip_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner_tmp = tempfile::tempdir().unwrap();
    make_zip(
        &inner_tmp.path().join("bonus-old.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":0,"description":"old"}}"#,
            ),
            ("assets/minecraft/textures/items/apple.png", b"old"),
        ],
    );
    let inner_bytes = fs::read(inner_tmp.path().join("bonus-old.zip")).unwrap();
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("bonus-old.zip", &inner_bytes),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("bonus-old.zip")).unwrap(), inner_bytes);
    let rescan = scan(&rp);
    let old = rescan
        .entries
        .iter()
        .find(|entry| entry.name == "bonus-old.zip")
        .unwrap();
    assert_eq!(old.category, Category::Normal);
    assert!(old.ignore.is_none());
    assert_eq!(result.outcomes[0].separated[0].name, "bonus-old.zip");
}

#[test]
fn folder_parent_separates_candidate_folder_without_repacking_it() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let parent = rp.join("Parent Folder");
    fs::create_dir_all(parent.join("assets/minecraft")).unwrap();
    fs::write(parent.join("pack.mcmeta"), MCMETA).unwrap();
    fs::write(parent.join("assets/minecraft/a.png"), b"parent").unwrap();
    let child = parent.join("bonus-modern");
    fs::create_dir_all(child.join("assets/minecraft")).unwrap();
    fs::write(
        child.join("pack.mcmeta"),
        br#"{"pack":{"pack_format":5,"description":"modern"}}"#,
    )
    .unwrap();
    fs::create_dir_all(child.join("assets/minecraft/textures/item")).unwrap();
    fs::write(
        child.join("assets/minecraft/textures/item/apple.png"),
        b"child",
    )
    .unwrap();

    let result = process(&opts_for(&tmp)).unwrap();

    assert!(rp.join("Parent Folder.zip").is_file());
    assert!(rp.join("bonus-modern").is_dir());
    assert_eq!(
        fs::read(rp.join("bonus-modern/assets/minecraft/textures/item/apple.png")).unwrap(),
        b"child"
    );
    assert_eq!(result.outcomes[0].separated[0].name, "bonus-modern");
}

#[test]
fn zip_parent_separates_candidate_folder_as_a_folder() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            (
                "bonus-modern/pack.mcmeta",
                br#"{"pack":{"pack_format":5,"description":"modern"}}"#,
            ),
            (
                "bonus-modern/assets/minecraft/textures/item/apple.png",
                b"child",
            ),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(result.outcomes[0].action, "converted");
    assert!(rp.join("Parent.zip").is_file());
    assert!(rp.join("bonus-modern").is_dir());
    assert_eq!(
        fs::read(rp.join("bonus-modern/assets/minecraft/textures/item/apple.png")).unwrap(),
        b"child"
    );
}

#[test]
fn case_variant_assets_folder_is_not_separated_as_a_pack() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let parent = rp.join("Parent");
    fs::create_dir_all(parent.join("assets/minecraft")).unwrap();
    fs::write(parent.join("pack.mcmeta"), MCMETA).unwrap();
    fs::write(parent.join("assets/minecraft/a.png"), b"parent").unwrap();
    let extra = parent.join("not-a-pack");
    fs::create_dir_all(extra.join("Assets/minecraft")).unwrap();
    fs::write(extra.join("pack.mcmeta"), MCMETA).unwrap();
    fs::write(extra.join("Assets/minecraft/a.png"), b"extra").unwrap();

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(result.outcomes[0].action, "converted");
    assert!(result.outcomes[0].separated.is_empty());
    assert!(!rp.join("not-a-pack").exists());
    assert!(tmp
        .path()
        .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Parent/not-a-pack")
        .is_dir());
}

#[test]
fn archive_container_matrix_is_separated_by_magic_or_supported_extension() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner_tmp = tempfile::tempdir().unwrap();
    make_zip(
        &inner_tmp.path().join("inner.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"inner"),
        ],
    );
    let zip_bytes = fs::read(inner_tmp.path().join("inner.zip")).unwrap();
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("magic-only", &zip_bytes),
            ("broken.ZIP", b"not a zip"),
            ("legacy.RAR", b"damaged rar"),
            ("seven.bin", b"7z\xbc\xaf\x27\x1cbytes"),
            ("leave.tar", b"tar bytes"),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    for name in ["magic-only", "broken.ZIP", "legacy.RAR", "seven.bin"] {
        assert!(rp.join(name).is_file(), "{name} should be separated");
    }
    assert!(!rp.join("leave.tar").exists());
    assert_eq!(result.outcomes[0].separated.len(), 4);
}

#[test]
fn separated_name_collision_uses_suffix_and_reports_actual_name() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    make_zip(&rp.join("bonus.zip"), &core_entries());
    let inner_tmp = tempfile::tempdir().unwrap();
    make_zip(
        &inner_tmp.path().join("bonus.zip"),
        &[
            (
                "pack.mcmeta",
                br#"{"pack":{"pack_format":5,"description":"modern"}}"#,
            ),
            ("assets/minecraft/textures/item/apple.png", b"modern"),
        ],
    );
    let modern = fs::read(inner_tmp.path().join("bonus.zip")).unwrap();
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("bonus.zip", &modern),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(fs::read(rp.join("bonus (1).zip")).unwrap(), modern);
    let parent = result
        .outcomes
        .iter()
        .find(|outcome| outcome.original_name == "Parent.zip")
        .unwrap();
    assert_eq!(parent.separated[0].name, "bonus (1).zip");
}

#[test]
fn separated_targets_reserve_names_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let inner_tmp = tempfile::tempdir().unwrap();
    make_zip(&inner_tmp.path().join("inner.zip"), &core_entries());
    let inner = fs::read(inner_tmp.path().join("inner.zip")).unwrap();
    make_zip(
        &rp.join("Parent.zip"),
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("bonus.zip", &inner),
            ("BONUS.ZIP", &inner),
        ],
    );

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(result.outcomes[0].action, "converted");
    assert_eq!(
        result.outcomes[0]
            .separated
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["bonus.zip", "BONUS (1).ZIP"]
    );
    assert!(rp.join("bonus.zip").is_file());
    assert!(rp.join("BONUS (1).ZIP").is_file());
}

#[test]
fn separation_prepare_failure_commits_neither_parent_nor_children() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    let path = rp.join("Parent.zip");
    let mut state = 0x1234_5678u32;
    let large: Vec<u8> = (0..20_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    make_zip(
        &path,
        &[
            ("pack.mcmeta", MCMETA),
            ("assets/minecraft/a.png", b"parent"),
            ("bonus/pack.mcmeta", MCMETA),
            ("bonus/assets/minecraft/big.bin", &large),
        ],
    );
    let mut original = fs::read(&path).unwrap();
    let marker = b"bonus/assets/minecraft/big.bin";
    let pos = original
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    for byte in &mut original[pos + marker.len() + 40..pos + marker.len() + 60] {
        *byte ^= 0xff;
    }
    fs::write(&path, &original).unwrap();

    let result = process(&opts_for(&tmp)).unwrap();

    assert_eq!(result.outcomes[0].action, "failed");
    assert!(!rp.join("Parent.zip").exists());
    assert!(!rp.join("bonus").exists());
    assert_eq!(
        fs::read(tmp.path().join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Parent.zip")).unwrap(),
        original
    );
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
            .join("plot_temp/Plot_2026-08-23_13.46.34/problematic_packs/Broken.zip")
            .exists(),
        "original preserved for manual recovery"
    );
    assert!(
        !rp.join("Broken.zip").exists(),
        "no half-product left in rp"
    );
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
