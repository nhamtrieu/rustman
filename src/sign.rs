//! Request signing for the AWS Signature Version 4 and HTTP Digest auth types, plus the small
//! hash functions they need (HMAC-SHA256, MD5). SHA-256 lives in `oauth2` (PKCE uses it).

use crate::oauth2::sha256;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(BLOCK + data.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(data);
    let mut outer = Vec::with_capacity(BLOCK + 32);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&sha256(&inner));
    sha256(&outer)
}

/// MD5 (RFC 1321). Only for HTTP Digest, which still defaults to it.
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4,
        11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// AWS Signature Version 4
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct AwsCredentials {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub service: String,
    pub session_token: String,
}

/// Payload hash for requests whose body is not known up front (multipart).
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// RFC 3986 encoding as AWS wants it: everything but `A-Z a-z 0-9 - _ . ~` is %XX.
fn aws_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Adds the SigV4 headers (`X-Amz-Date`, `X-Amz-Content-Sha256`, `X-Amz-Security-Token`,
/// `Authorization`) to `headers`. `amz_date` is `YYYYMMDDTHHMMSSZ` in UTC.
pub fn aws_sign(method: &str, url: &str, headers: &mut Vec<(String, String)>, payload_hash: &str, creds: &AwsCredentials, amz_date: &str) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|e| format!("AWS Signature: invalid URL '{url}': {e}"))?;
    let host = match (u.host_str(), u.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        (None, _) => return Err("AWS Signature: the URL has no host".into()),
    };
    let region = if creds.region.trim().is_empty() { "us-east-1" } else { creds.region.trim() };
    let service = creds.service.trim();
    if service.is_empty() {
        return Err("AWS Signature: set the Service Name (for example execute-api or s3)".into());
    }
    if creds.access_key.trim().is_empty() || creds.secret_key.is_empty() {
        return Err("AWS Signature: the access key and secret key are required".into());
    }

    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("x-amz-date") && !k.eq_ignore_ascii_case("authorization"));
    headers.push(("X-Amz-Date".into(), amz_date.into()));
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-amz-content-sha256")) {
        headers.push(("X-Amz-Content-Sha256".into(), payload_hash.into()));
    }
    if !creds.session_token.is_empty() && !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-amz-security-token")) {
        headers.push(("X-Amz-Security-Token".into(), creds.session_token.clone()));
    }
    let authorization = aws_authorization(method, &u, &host, headers, payload_hash, creds.access_key.trim(), &creds.secret_key, region, service, amz_date);
    headers.push(("Authorization".into(), authorization));
    Ok(())
}

/// The `Authorization` header value. Signs `host`, `content-type` and every `x-amz-*` header.
#[allow(clippy::too_many_arguments)]
fn aws_authorization(method: &str, u: &url::Url, host: &str, headers: &[(String, String)], payload_hash: &str, access_key: &str, secret_key: &str, region: &str, service: &str, amz_date: &str) -> String {
    // Path: already percent-encoded once by the URL parser. S3 signs it as is; every other
    // service encodes each segment again.
    let path = if u.path().is_empty() { "/" } else { u.path() };
    let canonical_uri = if service == "s3" { path.to_string() } else { path.split('/').map(|seg| aws_encode(seg, false)).collect::<Vec<_>>().join("/") };

    let mut query: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (aws_encode(&k, false), aws_encode(&v, false))).collect();
    query.sort();
    let canonical_query = query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

    let mut signed: Vec<(String, String)> = vec![("host".into(), host.to_string())];
    for (k, v) in headers {
        let name = k.to_ascii_lowercase();
        if name == "content-type" || name.starts_with("x-amz-") {
            signed.push((name, v.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
    }
    signed.sort();
    let canonical_headers: String = signed.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = signed.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");

    let canonical_request = format!("{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let date = &amz_date[..8.min(amz_date.len())];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", sha256_hex(canonical_request.as_bytes()));

    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));
    format!("AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}")
}

// ---------------------------------------------------------------------------
// HTTP Digest (RFC 7616 / RFC 2617)
// ---------------------------------------------------------------------------

/// A `WWW-Authenticate: Digest ...` challenge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DigestChallenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// `MD5` when the server does not say.
    pub algorithm: String,
    /// Offered qop values (`auth`, `auth-int`).
    pub qop: Vec<String>,
}

/// Finds a supported Digest challenge among `WWW-Authenticate` header values. A server may offer
/// several (SHA-256 and MD5); the first supported one wins, as the server lists its preference first.
pub fn parse_digest_challenge<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<DigestChallenge> {
    for v in values {
        // One header can hold several challenges; split before each `Digest` scheme token.
        let mut rest = v;
        while let Some(pos) = find_ci(rest, "digest ") {
            rest = &rest[pos + 7..];
            let params = parse_auth_params(rest);
            let get = |k: &str| params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
            let algorithm = get("algorithm").unwrap_or_else(|| "MD5".into());
            let supported = ["md5", "md5-sess", "sha-256", "sha-256-sess"].contains(&algorithm.to_ascii_lowercase().as_str());
            if let (Some(nonce), true) = (get("nonce"), supported) {
                return Some(DigestChallenge {
                    realm: get("realm").unwrap_or_default(),
                    nonce,
                    opaque: get("opaque"),
                    algorithm,
                    qop: get("qop").map(|q| q.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
                });
            }
        }
    }
    None
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(needle)
}

/// `key=value, key="quoted, value"` pairs, stopping at the next auth scheme.
fn parse_auth_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    loop {
        while i < b.len() && (b[i] == b' ' || b[i] == b',') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i] != b'=' && b[i] != b',' && b[i] != b' ' {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            break; // end, or the next scheme name
        }
        let key = s[start..i].to_string();
        i += 1;
        let value = if i < b.len() && b[i] == b'"' {
            i += 1;
            let mut v = String::new();
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' && i + 1 < b.len() {
                    i += 1;
                }
                v.push(b[i] as char);
                i += 1;
            }
            i += 1;
            v
        } else {
            let vs = i;
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            s[vs..i].trim().to_string()
        };
        out.push((key, value));
    }
    out
}

/// The `Authorization: Digest ...` header for one request. `uri` is the path and query sent;
/// `body` is only used for `qop=auth-int`; `cnonce` should be random per request.
#[allow(clippy::too_many_arguments)]
pub fn digest_authorization(ch: &DigestChallenge, username: &str, password: &str, method: &str, uri: &str, body: &[u8], cnonce: &str, nc: u32) -> String {
    let algo = ch.algorithm.to_ascii_lowercase();
    let h = |s: &str| -> String { if algo.starts_with("sha-256") { sha256_hex(s.as_bytes()) } else { hex(&md5(s.as_bytes())) } };
    let mut ha1 = h(&format!("{username}:{}:{password}", ch.realm));
    if algo.ends_with("-sess") {
        ha1 = h(&format!("{ha1}:{}:{cnonce}", ch.nonce));
    }
    let qop = if ch.qop.iter().any(|q| q == "auth") {
        Some("auth")
    } else if ch.qop.iter().any(|q| q == "auth-int") {
        Some("auth-int")
    } else {
        None
    };
    let ha2 = match qop {
        Some("auth-int") => {
            let body_hash = if algo.starts_with("sha-256") { sha256_hex(body) } else { hex(&md5(body)) };
            h(&format!("{method}:{uri}:{body_hash}"))
        }
        _ => h(&format!("{method}:{uri}")),
    };
    let nc = format!("{nc:08x}");
    let response = match qop {
        Some(q) => h(&format!("{ha1}:{}:{nc}:{cnonce}:{q}:{ha2}", ch.nonce)),
        None => h(&format!("{ha1}:{}:{ha2}", ch.nonce)),
    };
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={}",
        quote(username),
        quote(&ch.realm),
        quote(&ch.nonce),
        quote(uri),
        ch.algorithm
    );
    if let Some(q) = qop {
        out.push_str(&format!(", qop={q}, nc={nc}, cnonce=\"{cnonce}\""));
    }
    out.push_str(&format!(", response=\"{response}\""));
    if let Some(o) = &ch.opaque {
        out.push_str(&format!(", opaque=\"{}\"", quote(o)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(&md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        let long = "1234567890".repeat(8);
        assert_eq!(hex(&md5(long.as_bytes())), "57edf4a22be3c955ac49da2e2107b67a");
    }

    #[test]
    fn hmac_sha256_rfc4231() {
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // Key longer than the block size (test case 6).
        let key = [0xaau8; 131];
        assert_eq!(
            hex(&hmac_sha256(&key, b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    const AK: &str = "AKIDEXAMPLE";
    const SK: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    #[test]
    fn aws_sigv4_test_suite_get_vanilla() {
        // From the AWS SigV4 test suite (get-vanilla).
        let u = url::Url::parse("https://example.amazonaws.com/").unwrap();
        let headers = vec![("X-Amz-Date".to_string(), "20150830T123600Z".to_string())];
        let auth = aws_authorization("GET", &u, "example.amazonaws.com", &headers, &sha256_hex(b""), AK, SK, "us-east-1", "service", "20150830T123600Z");
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn aws_sigv4_iam_example() {
        // The IAM ListUsers example from the AWS General Reference.
        let u = url::Url::parse("https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08").unwrap();
        let headers = vec![
            ("Content-Type".to_string(), "application/x-www-form-urlencoded; charset=utf-8".to_string()),
            ("X-Amz-Date".to_string(), "20150830T123600Z".to_string()),
        ];
        let auth = aws_authorization("GET", &u, "iam.amazonaws.com", &headers, &sha256_hex(b""), AK, SK, "us-east-1", "iam", "20150830T123600Z");
        assert!(auth.ends_with("SignedHeaders=content-type;host;x-amz-date, Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"), "{auth}");
    }

    #[test]
    fn aws_sign_adds_headers() {
        let creds = AwsCredentials { access_key: AK.into(), secret_key: SK.into(), region: String::new(), service: "execute-api".into(), session_token: "TOK".into() };
        let mut h = vec![("Authorization".to_string(), "stale".to_string())];
        aws_sign("POST", "https://abc.execute-api.eu-west-1.amazonaws.com:8443/prod/a b?z=1&a=x y", &mut h, &sha256_hex(b"{}"), &creds, "20240101T000000Z").unwrap();
        let get = |k: &str| h.iter().filter(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).collect::<Vec<_>>();
        assert_eq!(get("authorization").len(), 1, "an old Authorization header is replaced");
        assert!(get("authorization")[0].contains("/20240101/us-east-1/execute-api/aws4_request"), "empty region means us-east-1");
        assert!(get("authorization")[0].contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token,"));
        assert_eq!(get("x-amz-security-token"), vec!["TOK"]);
        let missing = AwsCredentials { service: String::new(), ..creds };
        assert!(aws_sign("GET", "https://x.io/", &mut Vec::new(), UNSIGNED_PAYLOAD, &missing, "20240101T000000Z").is_err());
    }

    #[test]
    fn digest_rfc2617_example() {
        let ch = parse_digest_challenge([r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#]).unwrap();
        assert_eq!(ch.qop, vec!["auth", "auth-int"]);
        assert_eq!(ch.algorithm, "MD5");
        let h = digest_authorization(&ch, "Mufasa", "Circle Of Life", "GET", "/dir/index.html", b"", "0a4f113b", 1);
        assert!(h.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{h}");
        assert!(h.contains("qop=auth, nc=00000001, cnonce=\"0a4f113b\""));
        assert!(h.ends_with(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));
    }

    #[test]
    fn digest_challenge_choice() {
        assert!(parse_digest_challenge(["Basic realm=\"x\""]).is_none());
        // Unsupported algorithm first, then a usable one in the same header.
        let ch = parse_digest_challenge([r#"Digest realm="r", nonce="n1", algorithm=SHA-512-256, Digest realm="r", nonce="n2", algorithm=SHA-256, qop="auth""#]).unwrap();
        assert_eq!((ch.nonce.as_str(), ch.algorithm.as_str()), ("n2", "SHA-256"));
        // No qop (RFC 2069 style).
        let ch = parse_digest_challenge(["Digest realm=\"r\", nonce=\"n\""]).unwrap();
        let h = digest_authorization(&ch, "u", "p", "GET", "/", b"", "c", 1);
        assert!(!h.contains("qop="));
    }
}
