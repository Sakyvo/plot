// Manual sanity: process a folder and print outcomes.
// Usage: cargo run -p engine --example process_real -- <resourcepacks> <plot_temp>
fn main() {
    let rp = std::env::args().nth(1).expect("resourcepacks path");
    let pt = std::env::args().nth(2).expect("plot_temp path");
    let opts = engine::ProcessOptions {
        resourcepacks: rp.clone().into(),
        plot_temp: pt.into(),
        run_dir_name: engine::default_run_dir_name(),
    };
    let report = engine::process(&opts).expect("process failed");
    for o in &report.outcomes {
        println!(
            "{}\t{}\t-> {}",
            o.action,
            o.original_name,
            o.products.join(", ")
        );
        if let Some(d) = &o.detail {
            println!("    detail: {d}");
        }
    }
    let after = engine::scan(std::path::Path::new(&rp));
    println!(
        "AFTER: normal={} nested={} folder={} bloated={} illegal={}",
        after.counts.normal,
        after.counts.nested,
        after.counts.folder,
        after.counts.bloated,
        after.counts.illegal
    );
}
