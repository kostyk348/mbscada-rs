//! Import a folder/file of .mbp/.mbw and print a report.
//! Usage: cargo run -p mb-engine --example importscan -- <path> [<path>...]

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: importscan <path> [<path>...]");
        std::process::exit(1);
    }
    let res = mb_engine::import::import_paths(paths);
    let ok = res.reports.iter().filter(|r| r.ok).count();
    let bad = res.reports.iter().filter(|r| !r.ok).count();

    for c in &res.connections {
        println!("connection: {} -> {}:{}", c.name, c.host, c.port);
    }
    println!("polls: {}  (reports ok {ok} / bad {bad})", res.polls.len());
    for (i, p) in res.polls.iter().enumerate().take(12) {
        println!(
            "  {:>2}. fc={} @{:<6} qty={:<4} slave={:<4} fmt={:?} note={:<6} labels={:<3} {}",
            i + 1,
            p.func,
            p.start,
            p.qty,
            p.slave,
            p.fmt,
            if p.notation.is_empty() { "dec" } else { &p.notation },
            p.labels.len(),
            p.name
        );
    }
        if res.polls.len() > 12 {
        println!("  ... +{} more", res.polls.len() - 12);
    }
    // show a couple of label keys/values to verify alignment
    for p in res.polls.iter().take(3) {
        let mut keys: Vec<_> = p.labels.iter().collect();
        keys.sort();
        let sample: Vec<String> = keys.iter().take(3).map(|(k, v)| format!("{k}={v}")).collect();
        println!("  labels[{}]: {}", p.name, sample.join(", "));
    }
    for r in res.reports.iter().filter(|r| !r.ok) {
        println!("  ERR {}: {}", r.path, r.message);
    }
}
