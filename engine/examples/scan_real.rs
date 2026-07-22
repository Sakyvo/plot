// Manual sanity harness: scan a real folder and print category counts.
// Usage: cargo run -p engine --example scan_real -- <path>
fn main() {
    let path = std::env::args().nth(1).expect("pass a folder path");
    let report = engine::scan(std::path::Path::new(&path));
    println!(
        "status={:?} total={} normal={} nested={} folder={} bloated={} illegal={}",
        report.status,
        report.entries.len(),
        report.counts.normal,
        report.counts.nested,
        report.counts.folder,
        report.counts.bloated,
        report.counts.illegal
    );
    for e in &report.entries {
        if e.category != engine::Category::Normal || !e.causes.is_empty() {
            println!("{:?}\t{:?}\t{}", e.category, e.causes, e.name);
        }
    }
}
