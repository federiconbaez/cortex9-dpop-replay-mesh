//! Minimal DPoP proofs (RFC 9449 section 4): compact JWS, `typ: dpop+jwt`,
//! `alg: ES256`, public key in the `jwk` header; claims `jti`, `htm`, `htu`,
//! `iat` and optionally `nonce`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use serde_json::{json, Value};

/// Method used by every request.
pub const HTM: &str = "GET";
/// Target URI used by every request.
pub const HTU: &str = "https://api.example/resource";

/// Claims extracted from a verified proof.
#[derive(Clone, Debug, PartialEq)]
pub struct Claims {
    pub jti: String,
    pub iat_ms: i64,
    pub nonce: Option<String>,
}

/// Create a proof. `iat_ms` is milliseconds; it is written as a
/// non-integer NumericDate in seconds.
pub fn create(key: &SigningKey, jti: &str, iat_ms: i64, nonce: Option<&str>) -> String {
    let ep = VerifyingKey::from(key).to_encoded_point(false);
    let header = json!({
        "typ": "dpop+jwt",
        "alg": "ES256",
        "jwk": {
            "kty": "EC",
            "crv": "P-256",
            "x": B64.encode(ep.x().expect("x")),
            "y": B64.encode(ep.y().expect("y")),
        }
    });
    let mut claims = json!({
        "jti": jti,
        "htm": HTM,
        "htu": HTU,
        "iat": iat_ms as f64 / 1000.0,
    });
    if let Some(n) = nonce {
        claims["nonce"] = json!(n);
    }
    let si = format!(
        "{}.{}",
        B64.encode(header.to_string()),
        B64.encode(claims.to_string())
    );
    let sig: Signature = key.sign(si.as_bytes());
    format!("{si}.{}", B64.encode(sig.to_bytes()))
}

/// Verify signature and the request-binding claims. Freshness, `jti` and
/// `nonce` checks are the caller's job.
pub fn verify(proof: &str) -> Result<Claims, &'static str> {
    let mut it = proof.split('.');
    let (h, p, s) = match (it.next(), it.next(), it.next(), it.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => return Err("format"),
    };
    let hv: Value =
        serde_json::from_slice(&B64.decode(h).map_err(|_| "b64")?).map_err(|_| "json")?;
    if hv["typ"] != "dpop+jwt" || hv["alg"] != "ES256" {
        return Err("header");
    }
    let jwk = &hv["jwk"];
    if jwk["kty"] != "EC" || jwk["crv"] != "P-256" {
        return Err("jwk");
    }
    let x = B64
        .decode(jwk["x"].as_str().ok_or("jwk")?)
        .map_err(|_| "jwk")?;
    let y = B64
        .decode(jwk["y"].as_str().ok_or("jwk")?)
        .map_err(|_| "jwk")?;
    let x: [u8; 32] = x.try_into().map_err(|_| "jwk")?;
    let y: [u8; 32] = y.try_into().map_err(|_| "jwk")?;
    let ep = p256::EncodedPoint::from_affine_coordinates(&x.into(), &y.into(), false);
    let vk = VerifyingKey::from_encoded_point(&ep).map_err(|_| "jwk")?;
    let sig = Signature::from_slice(&B64.decode(s).map_err(|_| "b64")?).map_err(|_| "sig")?;
    vk.verify(format!("{h}.{p}").as_bytes(), &sig)
        .map_err(|_| "sig")?;
    let pv: Value =
        serde_json::from_slice(&B64.decode(p).map_err(|_| "b64")?).map_err(|_| "json")?;
    if pv["htm"] != HTM || pv["htu"] != HTU {
        return Err("binding");
    }
    let jti = pv["jti"].as_str().ok_or("jti")?.to_string();
    let iat = pv["iat"].as_f64().ok_or("iat")?;
    Ok(Claims {
        jti,
        iat_ms: (iat * 1000.0).round() as i64,
        nonce: pv["nonce"].as_str().map(str::to_string),
    })
}
