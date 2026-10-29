//! Sweep N x skew x mode and write CSV + JSON. `--check` exits 1 if a
//! pre-registered control fails.

use dpop_replay_mesh::mesh::{run, Config, Mode};
use std::fmt::Write as _;

fn arg(name: &str, def: &str) -> String {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
        .unwrap_or_else(|| def.to_string())
}

fn main() {
    let requests: usize = arg("--requests", "1000").parse().unwrap();
    let seed: u64 = arg("--seed", "42").parse().unwrap();
    let csv_path = arg("--csv", "results/sweep.csv");
    let check = std::env::args().any(|a| a == "--check");
    let nodes = [1usize, 2, 4, 8, 16];
    let skews = [0i64, 1000, 2000, 5000];
    let mut csv = String::from("nodes,skew_ms,mode,legit_ok,legit_sent,nonce_retries,replays_sent,replays_accepted,replay_pct,p50_us,p99_us\n");
    let mut fails = Vec::new();
    let mut p99: std::collections::HashMap<(usize, i64, &str), f64> = Default::default();
    for (ni, &n) in nodes.iter().enumerate() {
        for (si, &s) in skews.iter().enumerate() {
            let modes = Mode::ALL.iter().copied().chain([Mode::LocalIatTtl]);
            for (mi, m) in modes.enumerate() {
                // Same seeds as the first run for the original five modes.
                let k = ((ni * skews.len() + si) * Mode::ALL.len() + mi) as u64 + 1;
                let k = if m == Mode::LocalIatTtl {
                    10_000 + k
                } else {
                    k
                };
                let cfg = Config {
                    nodes: n,
                    skew_ms: s,
                    mode: m,
                    window_ms: 10_000,
                    requests,
                    interval_ms: 10,
                    seed: seed.wrapping_mul(1000) + k,
                };
                let st = run(cfg);
                let line = format!(
                    "{n},{s},{},{},{},{},{},{},{:.2},{:.1},{:.1}",
                    m.name(),
                    st.legit_ok,
                    st.legit_sent,
                    st.legit_retries,
                    st.replays_sent,
                    st.replays_accepted,
                    100.0 * st.replay_rate(),
                    st.pct(50.0),
                    st.pct(99.0)
                );
                println!("{line}");
                p99.insert((n, s, m.name()), st.pct(99.0));
                let _ = writeln!(csv, "{line}");
                if st.legit_ok != st.legit_sent {
                    fails.push(format!("{line}: legit requests rejected"));
                }
                match m {
                    Mode::NoCheck if st.replays_accepted == 0 => {
                        fails.push(format!("{line}: negative control let no replay through"))
                    }
                    Mode::SharedTtlWS | Mode::Nonce if st.replays_accepted != 0 => {
                        fails.push(format!("{line}: replays accepted"))
                    }
                    Mode::LocalIatTtl if n == 1 && st.replays_accepted != 0 => fails.push(format!(
                        "{line}: single-node iat-TTL cache accepted replays"
                    )),
                    _ => {}
                }
            }
        }
    }
    if let Some(dir) = std::path::Path::new(&csv_path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&csv_path, csv).expect("write csv");
    let mut ratio = String::from("nodes,skew_ms,p99_nonce_over_local,p99_nonce_over_shared_ttl_w_plus_skew,nonce_under_2x_local\n");
    for &n in &nodes {
        for &s in &skews {
            let r1 = p99[&(n, s, "nonce")] / p99[&(n, s, "local")];
            let r2 = p99[&(n, s, "nonce")] / p99[&(n, s, "shared_ttl_w_plus_skew")];
            let _ = writeln!(ratio, "{n},{s},{r1:.2},{r2:.2},{}", r1 < 2.0);
        }
    }
    let ratio_path = std::path::Path::new(&csv_path).with_file_name("latency_ratio.csv");
    std::fs::write(&ratio_path, &ratio).expect("write ratio");
    print!("{ratio}");
    for f in &fails {
        eprintln!("CHECK FAIL: {f}");
    }
    if check && !fails.is_empty() {
        std::process::exit(1);
    }
}
