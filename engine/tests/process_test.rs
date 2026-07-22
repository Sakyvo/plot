mod common;

use common::{core_entries, make_zip};
use engine::{process, ProcessOptions};
use std::fs;

fn opts_for(tmp: &tempfile::TempDir) -> ProcessOptions {
    ProcessOptions {
        resourcepacks: tmp.path().join("rp"),
        plot_temp: tmp.path().join("plot_temp"),
    }
}

fn setup(tmp: &tempfile::TempDir) {
    fs::create_dir_all(tmp.path().join("rp")).unwrap();
}

#[test]
fn illegal_entries_are_moved_into_illegal_packs() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    fs::write(rp.join("garbage.txt"), b"not a pack").unwrap();
    make_zip(&rp.join("good.zip"), &core_entries());

    let report = process(&opts_for(&tmp)).unwrap();

    assert!(!rp.join("garbage.txt").exists(), "original should be gone");
    assert!(tmp
        .path()
        .join("plot_temp/illegal_packs/garbage.txt")
        .exists());
    assert!(rp.join("good.zip").exists(), "normal pack untouched");
    let outcome = report
        .outcomes
        .iter()
        .find(|o| o.original_name == "garbage.txt")
        .unwrap();
    assert_eq!(outcome.action, "moved_to_illegal");
}

#[test]
fn moving_a_second_illegal_with_the_same_name_gets_a_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");

    fs::write(rp.join("junk.bin"), b"one").unwrap();
    process(&opts_for(&tmp)).unwrap();
    fs::write(rp.join("junk.bin"), b"two").unwrap();
    process(&opts_for(&tmp)).unwrap();

    let quarantine = tmp.path().join("plot_temp/illegal_packs");
    assert!(quarantine.join("junk.bin").exists());
    assert!(quarantine.join("junk (1).bin").exists());
}

#[test]
fn an_unwritable_plot_temp_location_fails_without_touching_anything() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    fs::write(rp.join("garbage.txt"), b"x").unwrap();
    // A file where plot_temp's parent should be makes creation impossible.
    fs::write(tmp.path().join("blocker"), b"file").unwrap();
    let opts = ProcessOptions {
        resourcepacks: rp.clone(),
        plot_temp: tmp.path().join("blocker/plot_temp"),
    };

    let result = process(&opts);

    assert!(result.is_err());
    assert!(rp.join("garbage.txt").exists(), "nothing moved on failure");
}

#[test]
fn progress_reports_each_handled_pack() {
    let tmp = tempfile::tempdir().unwrap();
    setup(&tmp);
    let rp = tmp.path().join("rp");
    fs::write(rp.join("a.txt"), b"x").unwrap();
    fs::write(rp.join("b.txt"), b"x").unwrap();

    let mut seen = Vec::new();
    engine::process_with_progress(&opts_for(&tmp), &mut |ev| {
        seen.push(ev.name.clone());
    })
    .unwrap();

    assert!(seen.contains(&"a.txt".to_string()));
    assert!(seen.contains(&"b.txt".to_string()));
}
