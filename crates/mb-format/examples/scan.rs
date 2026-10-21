//! Scan a directory of real `.mbp`/`.mbw` fixtures and report parse results.
//!
//! Usage: `cargo run -p mb-format --example scan -- <dir>`

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".".to_string());
    let mut total = 0usize;
    let mut ok = 0usize;
    let mut sum_conf = 0.0f64;

    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("cannot read {dir}: {e}");
            std::process::exit(1);
        }
    };

    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();

    for path in entries {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "mbp" {
            total += 1;
            match mb_format::parse_mbp_file(&path) {
                Ok(t) => {
                    ok += 1;
                    sum_conf += t.confidence as f64;
                    println!(
                        "MBP ok  fc={:>2} addr={:>6} qty={:>4} per={:>6} slave={:>3} rows={:>3} conf={:.1}  {}",
                        t.func, t.addr, t.qty, t.period_ms, t.slave, t.rows.len(), t.confidence, t.name
                    );
                }
                Err(e) => println!("MBP ERR {e:?}  {}", path.display()),
            }
        } else if ext == "mbw" {
            total += 1;
            match mb_format::parse_mbw_file(&path) {
                Ok(w) => {
                    ok += 1;
                    println!(
                        "MBW ok  host={} port={} tables={}  {}",
                        w.host,
                        w.port,
                        w.tables.len(),
                        w.path
                    );
                    for t in &w.tables {
                        println!("        -> {} exists={}", t.name, t.exists);
                    }
                }
                Err(e) => println!("MBW ERR {e:?}  {}", path.display()),
            }
        }
    }

    println!("\n=== {ok}/{total} parsed ok ===");
    if ok > 0 {
        println!("mean confidence (mbp): {:.3}", sum_conf / ok as f64);
    }
}
