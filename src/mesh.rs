//! Client, N resource-server nodes and an optional shared jti cache, all
//! talking line-based text over real loopback TCP.

use crate::proof;
use p256::ecdsa::SigningKey;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Replay-protection strategy of the nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No jti check at all (negative control: replays must get through).
    NoCheck,
    /// Each node keeps its own jti cache.
    Local,
    /// One shared jti cache, entries live `window` ms on the cache clock.
    SharedTtlW,
    /// One shared jti cache, entries live `window + skew` ms.
    SharedTtlWS,
    /// Each node keeps its own jti cache, entries expire at `iat + window`
    /// on the node clock (added after the first run, see PREREGISTRO.md).
    LocalIatTtl,
    /// Server-issued single-use nonces (RFC 9449 section 9), tracked by the
    /// node that issued them.
    Nonce,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::NoCheck,
        Mode::Local,
        Mode::SharedTtlW,
        Mode::SharedTtlWS,
        Mode::Nonce,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Mode::NoCheck => "no_check",
            Mode::Local => "local",
            Mode::SharedTtlW => "shared_ttl_w",
            Mode::SharedTtlWS => "shared_ttl_w_plus_skew",
            Mode::Nonce => "nonce",
            Mode::LocalIatTtl => "local_ttl_iat",
        }
    }
    fn shared(self) -> bool {
        matches!(self, Mode::SharedTtlW | Mode::SharedTtlWS)
    }
}

/// One experiment.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub nodes: usize,
    /// Each node clock is offset by a value drawn uniformly in [-skew, +skew].
    pub skew_ms: i64,
    pub mode: Mode,
    /// Freshness window on `iat`, both sides.
    pub window_ms: i64,
    /// Legitimate requests; one replay is attempted per accepted request.
    pub requests: usize,
    /// Simulated time between legitimate requests.
    pub interval_ms: i64,
    pub seed: u64,
}

/// Results of one experiment.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub legit_sent: usize,
    pub legit_ok: usize,
    pub legit_retries: usize,
    pub replays_sent: usize,
    pub replays_accepted: usize,
    /// Client-observed latency of accepted legitimate requests, in
    /// microseconds, including proof creation and any nonce retry.
    pub latencies_us: Vec<f64>,
}

impl Stats {
    pub fn pct(&self, p: f64) -> f64 {
        if self.latencies_us.is_empty() {
            return f64::NAN;
        }
        let mut v = self.latencies_us.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((p / 100.0) * (v.len() as f64 - 1.0)).round() as usize;
        v[idx]
    }
    pub fn replay_rate(&self) -> f64 {
        if self.replays_sent == 0 {
            0.0
        } else {
            self.replays_accepted as f64 / self.replays_sent as f64
        }
    }
}

fn cache_server(l: TcpListener, conns: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let map: Arc<Mutex<HashMap<String, i64>>> = Arc::default();
        let mut hs = Vec::new();
        for _ in 0..conns {
            let (s, _) = l.accept().expect("cache accept");
            let map = map.clone();
            hs.push(thread::spawn(move || {
                let mut w = s.try_clone().expect("clone");
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    // SETNX <jti> <expiry_ms> <now_ms>
                    let p: Vec<&str> = line.split(' ').collect();
                    let (jti, exp, now) = (
                        p[1],
                        p[2].parse::<i64>().unwrap(),
                        p[3].parse::<i64>().unwrap(),
                    );
                    let mut m = map.lock().unwrap();
                    let dup = matches!(m.get(jti), Some(&e) if e > now);
                    if !dup {
                        m.insert(jti.to_string(), exp);
                    }
                    drop(m);
                    let _ = w.write_all(if dup { b"DUP\n" } else { b"NEW\n" });
                }
            }));
        }
        for h in hs {
            let _ = h.join();
        }
    })
}

struct Node {
    id: usize,
    offset_ms: i64,
    cfg: Config,
    cache: Option<(TcpStream, BufReader<TcpStream>)>,
    jtis: HashMap<String, i64>,
    nonces: HashMap<String, i64>,
    counter: u64,
    rng: ChaCha20Rng,
}

impl Node {
    fn fresh_nonce(&mut self, now: i64) -> String {
        self.counter += 1;
        let n = format!(
            "{}.{}.{:016x}",
            self.id,
            self.counter,
            self.rng.gen::<u64>()
        );
        self.nonces.insert(n.clone(), now);
        n
    }

    fn handle(&mut self, true_ms: i64, p: &str) -> String {
        let now = true_ms + self.offset_ms;
        let c = match proof::verify(p) {
            Ok(c) => c,
            Err(e) => return format!("ERR {e}"),
        };
        if (now - c.iat_ms).abs() > self.cfg.window_ms {
            return "ERR iat".into();
        }
        match self.cfg.mode {
            Mode::NoCheck => "OK -".into(),
            Mode::Local => {
                if matches!(self.jtis.get(&c.jti), Some(&e) if e > now) {
                    return "ERR replay".into();
                }
                self.jtis.insert(c.jti, now + self.cfg.window_ms);
                "OK -".into()
            }
            Mode::LocalIatTtl => {
                // Keep the jti as long as the iat window can accept it.
                if matches!(self.jtis.get(&c.jti), Some(&e) if e >= now) {
                    return "ERR replay".into();
                }
                self.jtis.insert(c.jti, c.iat_ms + self.cfg.window_ms);
                "OK -".into()
            }
            Mode::SharedTtlW | Mode::SharedTtlWS => {
                let ttl = if self.cfg.mode == Mode::SharedTtlW {
                    self.cfg.window_ms
                } else {
                    self.cfg.window_ms + self.cfg.skew_ms
                };
                let (w, r) = self.cache.as_mut().expect("cache");
                // The cache clock is the true clock.
                writeln!(w, "SETNX {} {} {}", c.jti, true_ms + ttl, true_ms).unwrap();
                let mut resp = String::new();
                r.read_line(&mut resp).unwrap();
                if resp.trim() == "DUP" {
                    "ERR replay".into()
                } else {
                    "OK -".into()
                }
            }
            Mode::Nonce => {
                let ok = match c.nonce.as_deref() {
                    Some(n) => match self.nonces.remove(n) {
                        Some(issued) => now - issued <= self.cfg.window_ms,
                        None => false,
                    },
                    None => false,
                };
                let next = self.fresh_nonce(now);
                if ok {
                    format!("OK {next}")
                } else {
                    format!("NONCE {next}")
                }
            }
        }
    }
}

fn node_server(
    l: TcpListener,
    mut node: Node,
    cache_addr: Option<std::net::SocketAddr>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Some(a) = cache_addr {
            let s = TcpStream::connect(a).expect("cache connect");
            s.set_nodelay(true).unwrap();
            let r = BufReader::new(s.try_clone().unwrap());
            node.cache = Some((s, r));
        }
        let (s, _) = l.accept().expect("node accept");
        s.set_nodelay(true).unwrap();
        let mut w = s.try_clone().unwrap();
        for line in BufReader::new(s).lines() {
            let Ok(line) = line else { break };
            let mut it = line.splitn(3, ' ');
            let _ = it.next();
            let t: i64 = it.next().unwrap().parse().unwrap();
            let p = it.next().unwrap();
            let resp = node.handle(t, p);
            let _ = writeln!(w, "{resp}");
        }
    })
}

struct Conn {
    w: TcpStream,
    r: BufReader<TcpStream>,
}

impl Conn {
    fn call(&mut self, t: i64, p: &str) -> String {
        writeln!(self.w, "REQ {t} {p}").unwrap();
        let mut s = String::new();
        self.r.read_line(&mut s).unwrap();
        s.trim_end().to_string()
    }
}

enum Ev {
    Legit(usize),
    Replay(String),
}

/// Run one experiment.
pub fn run(cfg: Config) -> Stats {
    let mut rng = ChaCha20Rng::seed_from_u64(cfg.seed);
    let cache = if cfg.mode.shared() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        Some((cache_server(l, cfg.nodes), a))
    } else {
        None
    };
    let mut handles = Vec::new();
    let mut conns = Vec::new();
    for id in 0..cfg.nodes {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let offset_ms = if cfg.skew_ms == 0 {
            0
        } else {
            rng.gen_range(-cfg.skew_ms..=cfg.skew_ms)
        };
        let node = Node {
            id,
            offset_ms,
            cfg,
            cache: None,
            jtis: HashMap::new(),
            nonces: HashMap::new(),
            counter: 0,
            rng: ChaCha20Rng::seed_from_u64(cfg.seed ^ ((id as u64 + 1) * 0x9e37)),
        };
        handles.push(node_server(l, node, cache.as_ref().map(|c| c.1)));
        let s = TcpStream::connect(a).unwrap();
        s.set_nodelay(true).unwrap();
        conns.push(Conn {
            r: BufReader::new(s.try_clone().unwrap()),
            w: s,
        });
    }
    let key = SigningKey::random(&mut rng);
    let mut st = Stats::default();
    let mut heap: BinaryHeap<Reverse<(i64, u64, usize)>> = BinaryHeap::new();
    let mut evs: Vec<Option<Ev>> = Vec::new();
    for i in 0..cfg.requests {
        evs.push(Some(Ev::Legit(i)));
        heap.push(Reverse((
            i as i64 * cfg.interval_ms,
            evs.len() as u64,
            evs.len() - 1,
        )));
    }
    let mut last_nonce: Option<String> = None;
    while let Some(Reverse((t, _, idx))) = heap.pop() {
        let node = rng.gen_range(0..cfg.nodes);
        match evs[idx].take().unwrap() {
            Ev::Legit(i) => {
                st.legit_sent += 1;
                let jti = format!("{}-{i}", cfg.seed);
                let start = Instant::now();
                let mut p = proof::create(&key, &jti, t, last_nonce.as_deref());
                let mut resp = conns[node].call(t, &p);
                let mut tries = 0;
                while let Some(n) = resp.strip_prefix("NONCE ") {
                    if tries == 2 {
                        break;
                    }
                    tries += 1;
                    st.legit_retries += 1;
                    p = proof::create(&key, &jti, t, Some(n));
                    resp = conns[node].call(t, &p);
                }
                let el = start.elapsed().as_secs_f64() * 1e6;
                if let Some(rest) = resp.strip_prefix("OK ") {
                    st.legit_ok += 1;
                    st.latencies_us.push(el);
                    if rest != "-" {
                        last_nonce = Some(rest.to_string());
                    }
                    let delay = rng.gen_range(0..=2 * cfg.window_ms);
                    evs.push(Some(Ev::Replay(p)));
                    heap.push(Reverse((t + delay, evs.len() as u64, evs.len() - 1)));
                } else if let Some(n) = resp.strip_prefix("NONCE ") {
                    last_nonce = Some(n.to_string());
                }
            }
            Ev::Replay(p) => {
                st.replays_sent += 1;
                if conns[node].call(t, &p).starts_with("OK ") {
                    st.replays_accepted += 1;
                }
            }
        }
    }
    drop(conns);
    for h in handles {
        let _ = h.join();
    }
    if let Some((h, _)) = cache {
        let _ = h.join();
    }
    st
}
