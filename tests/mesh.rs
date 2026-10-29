use dpop_replay_mesh::mesh::{run, Config, Mode};
use dpop_replay_mesh::proof;
use p256::ecdsa::SigningKey;
use rand::SeedableRng;

fn key() -> SigningKey {
    SigningKey::random(&mut rand_chacha::ChaCha20Rng::seed_from_u64(7))
}

#[test]
fn proof_roundtrip() {
    let p = proof::create(&key(), "j1", 1_700_000_000_123, Some("n1"));
    let c = proof::verify(&p).unwrap();
    assert_eq!(c.jti, "j1");
    assert_eq!(c.iat_ms, 1_700_000_000_123);
    assert_eq!(c.nonce.as_deref(), Some("n1"));
}

#[test]
fn tampered_proof_rejected() {
    let p = proof::create(&key(), "j1", 1_000, None);
    let mut parts: Vec<String> = p.split('.').map(String::from).collect();
    // Swap the payload for another one signed with nothing.
    let other = proof::create(&key(), "j2", 1_000, None);
    parts[1] = other.split('.').nth(1).unwrap().to_string();
    assert_eq!(proof::verify(&parts.join(".")), Err("sig"));
    assert_eq!(proof::verify("a.b"), Err("format"));
}

fn small(mode: Mode, nodes: usize, skew_ms: i64) -> Config {
    Config {
        nodes,
        skew_ms,
        mode,
        window_ms: 10_000,
        requests: 60,
        interval_ms: 10,
        seed: 3,
    }
}

#[test]
fn no_check_lets_replays_through() {
    let s = run(small(Mode::NoCheck, 4, 0));
    assert_eq!(s.legit_ok, 60);
    assert!(s.replays_accepted > 0);
}

#[test]
fn shared_cache_with_skew_margin_and_nonce_block_replays() {
    for m in [Mode::SharedTtlWS, Mode::Nonce] {
        let s = run(small(m, 4, 5000));
        assert_eq!(s.legit_ok, 60, "{m:?}");
        assert_eq!(s.replays_accepted, 0, "{m:?}");
    }
}

#[test]
fn single_node_local_cache_blocks_replays_without_skew() {
    let s = run(small(Mode::Local, 1, 0));
    assert_eq!(s.legit_ok, 60);
    assert_eq!(s.replays_accepted, 0);
}

#[test]
fn single_node_iat_ttl_cache_blocks_replays_with_skew() {
    for skew in [1000, 2000, 5000] {
        let s = run(small(Mode::LocalIatTtl, 1, skew));
        assert_eq!(s.replays_accepted, 0, "skew {skew}");
    }
}

#[test]
fn percentile_of_empty_is_nan() {
    let s = dpop_replay_mesh::mesh::Stats::default();
    assert!(s.pct(99.0).is_nan());
}
