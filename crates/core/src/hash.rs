//! xxh3-128 hashing and the in-file processed marker (DESIGN §4, §6.6,
//! §13.15).
//!
//! All non-cryptographic hashing in Transcodarr is **xxh3-128** (one
//! width, no branching decision — §13.15):
//!
//! - **Sample hash** — over three 256 KB windows (head, middle, tail) of
//!   the file's bytes; a secondary change signal (§4).
//! - **Operation fingerprint** — over the canonical JSON of the matched
//!   step's *authored* operation; the value embedded in files (§6.6).
//!
//! Both digests are 32-character lowercase hex strings. These are change
//! signals, not security boundaries.

use serde_json::Value;

/// Bytes per sample window (§4, §13.15).
pub const SAMPLE_WINDOW: u64 = 256 * 1024;

/// Marker version embedded in in-file markers (§6.6).
pub const MARKER_VERSION: u8 = 1;

/// xxh3-128 over a byte slice, as a 32-character lowercase hex string.
#[must_use]
pub fn xxh3_128_hex(data: &[u8]) -> String {
    format!("{:032x}", twox_hash::XxHash3_128::oneshot(data))
}

/// The sample windows for a file of `size` bytes, in hash order (§4):
/// 256 KB at the head, middle, and tail. Files at or below the combined
/// window size hash their entire content (one window) — nothing is lost
/// and no seek is needed.
#[must_use]
pub fn sample_windows(size: u64) -> Vec<(u64, u64)> {
    if size <= 3 * SAMPLE_WINDOW {
        return vec![(0, size)];
    }
    vec![
        (0, SAMPLE_WINDOW),
        (size / 2 - SAMPLE_WINDOW / 2, SAMPLE_WINDOW),
        (size - SAMPLE_WINDOW, SAMPLE_WINDOW),
    ]
}

/// Sample hash over windowed byte chunks in `sample_windows` order (§4).
#[must_use]
pub fn sample_hash(windows: &[(u64, u64)], chunks: &[Vec<u8>]) -> String {
    // Reassemble in window order (the reader fills chunks in the same
    // order); a single hash over the concatenation keeps the digest
    // independent of window boundaries.
    let total: u64 = windows.iter().map(|(_, l)| *l).sum();
    let mut buf = Vec::with_capacity(total as usize);
    for c in chunks {
        buf.extend_from_slice(c);
    }
    xxh3_128_hex(&buf)
}

/// Operation fingerprint: xxh3-128 over the canonical JSON of the
/// authored operation (§6.6). This is the only input to the in-file
/// marker — the canonical form is pinned end-to-end by a CI fixture so
/// a canonicalization change fails the build instead of invalidating a
/// library of markers.
#[must_use]
pub fn fingerprint(op: &Value) -> String {
    let json = canonical_json(op);
    xxh3_128_hex(json.as_bytes())
}

/// Canonical JSON: keys sorted at every nesting level, no whitespace,
/// explicit string escaping, Rust's deterministic number formatting.
///
/// Deliberately independent of `serde_json`'s container ordering (which
/// depends on the `preserve_order` feature in *any* crate of the tree):
/// the fingerprint must be byte-stable across releases and platforms.
#[must_use]
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_json_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The marker string written into an output file (§6.6):
/// `transcodarr:t<version>:<32-hex fingerprint>`.
#[must_use]
pub fn marker_string(fingerprint: &str) -> String {
    format!("transcodarr:t{MARKER_VERSION}:{fingerprint}")
}

/// Parse a format-tag value into a marker fingerprint.
///
/// Accepts the value from either carrier location: the custom
/// `transcodarr` tag (MKV/WebM) or the `comment` tag (MP4/MOV) — the
/// grammar check is the discriminator, so a user comment that does not
/// match is simply not a marker.
#[must_use]
pub fn parse_marker(value: &str) -> Option<&str> {
    let rest = value.strip_prefix("transcodarr:t")?;
    let (version, fingerprint) = rest.split_once(':')?;
    if version == MARKER_VERSION.to_string()
        && fingerprint.len() == 32
        && fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
    {
        Some(fingerprint)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Known-answer pins (§13.15): fixture input → exact digest. These
    /// are the CI guard against canonicalization drift across releases —
    /// if any of these change, the build fails instead of a library of
    /// in-file markers silently invalidating.
    // Known-answer pins, verified against the reference implementation
    // (xxhsum 0.8.3, -H2) on the same inputs.
    const KAT_EMPTY: &str = "99aa06d3014798d86001c324468d497f";
    const KAT_A: &str = "a96faf705af16834e6c632b61e964e1f";
    const KAT_OBJECT: &str = "e02de5863dafb8f60dfe6f7343dc2fe8";
    const KAT_NESTED: &str = "d50129ecb6365668c92509e2ddc455a0";

    fn kat(input: &str) -> String {
        xxh3_128_hex(input.as_bytes())
    }

    #[test]
    fn xxh3_128_known_answers() {
        assert_eq!(kat(""), KAT_EMPTY);
        assert_eq!(kat("a"), KAT_A);
        assert_eq!(kat("{\"a\":\"x\",\"b\":1}"), KAT_OBJECT);
        assert_eq!(
            xxh3_128_hex(r#"{"a":{"b":null,"y":[3,1]},"z":1}"#.as_bytes()),
            KAT_NESTED
        );
    }

    #[test]
    fn sample_windows_cover_head_middle_tail() {
        let w = sample_windows(100 * 1024 * 1024);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0], (0, SAMPLE_WINDOW));
        assert_eq!(w[1], (50 * 1024 * 1024 - 128 * 1024, SAMPLE_WINDOW));
        assert_eq!(w[2], (100 * 1024 * 1024 - 256 * 1024, SAMPLE_WINDOW));
    }

    #[test]
    fn sample_windows_small_file_is_whole_file() {
        assert_eq!(sample_windows(0), vec![(0, 0)]);
        assert_eq!(sample_windows(768 * 1024), vec![(0, 768 * 1024)]);
        assert_eq!(
            sample_windows(768 * 1024 + 1),
            vec![
                (0, SAMPLE_WINDOW),
                (SAMPLE_WINDOW, SAMPLE_WINDOW),
                (768 * 1024 + 1 - SAMPLE_WINDOW, SAMPLE_WINDOW),
            ]
        );
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let v = json!({"z": 1, "a": {"y": [3, 1], "b": null}});
        assert_eq!(canonical_json(&v), r#"{"a":{"b":null,"y":[3,1]},"z":1}"#);
        // Key order in the source never leaks into the fingerprint.
        let v2 = json!({"a": {"b": null, "y": [3, 1]}, "z": 1});
        assert_eq!(canonical_json(&v), canonical_json(&v2));
        assert_eq!(
            fingerprint(&v),
            xxh3_128_hex(r#"{"a":{"b":null,"y":[3,1]},"z":1}"#.as_bytes())
        );
    }

    #[test]
    fn canonical_json_escapes_strings() {
        use serde_json::from_str;
        // Built from unambiguous JSON text (no macro double-encoding):
        // the value is q, quote, backslash, newline — and canonical
        // re-emission must be byte-identical.
        let v: Value = from_str(r#"{"s":"q\"\\n"}"#).unwrap();
        assert_eq!(canonical_json(&v), r#"{"s":"q\"\\n"}"#);
        let v2: Value = from_str(r#"{"s":"a\nb"}"#).unwrap();
        assert_eq!(canonical_json(&v2), r#"{"s":"a\nb"}"#);
    }

    #[test]
    fn marker_round_trip_and_rejection() {
        let fp = "0123456789abcdef0123456789abcdef";
        let m = marker_string(fp);
        assert_eq!(m, "transcodarr:t1:0123456789abcdef0123456789abcdef");
        assert_eq!(parse_marker(&m), Some(fp));
        // Not a marker:
        assert_eq!(parse_marker("a comment about life"), None);
        assert_eq!(
            parse_marker("transcodarr:t99:0123456789abcdef0123456789abcdef"),
            None
        );
        assert_eq!(parse_marker("transcodarr:t1:zzz"), None);
        assert_eq!(
            parse_marker("transcodarr:t1:0123456789abcdef0123456789abcde"),
            None
        );
    }
}
