//! I/O-bound fact extraction: run `ffprobe` and map its JSON into a
//! pure `FileFacts` (the unit of evaluation, DESIGN §8).
//!
//! `map_facts` is pure and unit-testable; `probe` (the
//! `FactExtractor` impl) shells out and delegates to it.

use std::path::Path;

use serde_json::Value;
use transcodarr_core::facts::{AudioTrack, FileFacts, Hdr, SubtitleTrack, VideoFacts};
use transcodarr_core::hash;
use transcodarr_core::registry::FactExtractor;

/// The ffprobe-backed fact extractor (registered in `Registry::v1()`
/// at server startup, DESIGN §10).
#[derive(Clone)]
pub struct FfprobeFactExtractor {
    pub path: String,
}

impl FfprobeFactExtractor {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl FactExtractor for FfprobeFactExtractor {
    fn key(&self) -> &'static str {
        "ffprobe"
    }

    fn probe(&self, path: &Path) -> std::io::Result<FileFacts> {
        let out = std::process::Command::new(&self.path)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output()
            .map_err(std::io::Error::other)?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ffprobe failed: {err}"),
            ));
        }
        let doc: Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        map_facts(path, &doc, None)
    }
}

impl FfprobeFactExtractor {
    /// Like `probe`, but names the container explicitly — for
    /// intermediate files whose file name carries no container
    /// extension (temp files, quarantined copies).
    pub fn probe_with_container(
        &self,
        path: &Path,
        container: Option<&str>,
    ) -> std::io::Result<FileFacts> {
        let out = std::process::Command::new(&self.path)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output()
            .map_err(std::io::Error::other)?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ffprobe failed: {err}"),
            ));
        }
        let doc: Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        map_facts(path, &doc, container)
    }
}

/// Map an ffprobe JSON document onto `FileFacts`.
///
/// Decisions:
/// - **Container** is derived from the file extension, not
///   `format_name` (which yields family strings like
///   "matroska,webm" rather than an extension).
/// - **Track indices** are per-type (0-based within their stream
///   type) — the per-type indices are what ffmpeg `-map` uses in
///   plan.rs.
/// - **Atmos** is a heuristic (eac3 with ≥8 channels; ffprobe does
///   not expose JOC metadata in v1).
/// - **HDR** comes from stream side data, collected as flags first
///   and decided afterwards: Dolby Vision → DolbyVision; "HDR10+"
///   (should ffprobe ever label it) → Hdr10Plus; mastering display
///   (± content light level) → Hdr10; content light level alone or
///   a PQ color transfer → Hlg (the safe default that never
///   triggers HDR-to-SDR on its own). A real HDR10 file carries
///   *both* mastering display and CLLI, so mastering display must
///   outrank CLLI — a per-entry "last one wins" chain misread those
///   as HLG when CLLI was listed first.
pub fn map_facts(
    path: &Path,
    doc: &Value,
    container_override: Option<&str>,
) -> std::io::Result<FileFacts> {
    let mut container = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Intermediate files (temp/quarantine names) carry no container
    // extension: the caller supplies the known container instead.
    if let Some(c) = container_override {
        container = c.to_ascii_lowercase();
    }

    let size = doc
        .get("format")
        .and_then(|f| f.get("size"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let duration_s = doc
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let bitrate_bps = doc
        .get("format")
        .and_then(|f| f.get("bit_rate"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    // The in-file processed marker (DESIGN §6.6) is read from the
    // format tags: the custom `transcodarr` key (MKV/WebM — the
    // muxer uppercases it, so lookups are case-insensitive) or the
    // `comment` slot (MP4/MOV). The value's grammar is the
    // discriminator: a user comment that does not match is simply
    // not a marker.
    let tags = doc
        .get("format")
        .and_then(|f| f.get("tags"))
        .and_then(Value::as_object);
    let tag = |name: &str| -> Option<String> {
        tags?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.as_str())
            .map(str::to_string)
    };
    let marker = tag("transcodarr")
        .or_else(|| tag("comment"))
        .filter(|s| hash::parse_marker(s).is_some());
    let comment = tag("comment");

    let mut video = None;
    let mut audio = Vec::new();
    let mut subtitles = Vec::new();
    let (mut aidx, mut sidx) = (0u32, 0u32);

    for s in doc
        .get("streams")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let r#type = s.get("codec_type").and_then(|v| v.as_str()).unwrap_or("");
        let codec = s
            .get("codec_name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let language = s
            .get("tags")
            .and_then(|t| t.get("language"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match r#type {
            "video" => {
                if video.is_some() {
                    continue; // one video stream is the model (v1)
                }
                let mut hdr = Hdr::None;
                let mut seen_dovi = false;
                let mut seen_hdr10plus = false;
                let mut seen_mastering = false;
                let mut seen_clli = false;
                for sd in s
                    .get("side_data_list")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    let name = sd
                        .get("side_data_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if name.contains("Dolby Vision") {
                        seen_dovi = true;
                    } else if name.contains("HDR10+") {
                        seen_hdr10plus = true;
                    } else if name.contains("Mastering display") {
                        seen_mastering = true;
                    } else if name.contains("Content light level") {
                        seen_clli = true;
                    }
                }
                // Decide after seeing all side data.
                if seen_dovi {
                    hdr = Hdr::DolbyVision;
                } else if seen_hdr10plus {
                    hdr = Hdr::Hdr10Plus;
                } else if seen_mastering {
                    hdr = Hdr::Hdr10;
                } else if seen_clli
                    || s.get("color_transfer").and_then(|v| v.as_str()) == Some("smpte2084")
                {
                    hdr = Hdr::Hlg;
                }
                video = Some(VideoFacts {
                    codec: codec.unwrap_or_default(),
                    profile: s
                        .get("profile")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    level: s
                        .get("level")
                        .and_then(|v| v.as_u64())
                        .map(|l| norm_level(l, codec_str(s))),
                    pixel_format: s
                        .get("pix_fmt")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    width: s.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    height: s.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    frame_rate: s
                        .get("avg_frame_rate")
                        .and_then(|v| v.as_str())
                        .and_then(parse_rate),
                    hdr,
                    bitrate_bps: s
                        .get("bit_rate")
                        .and_then(|v| v.as_str())
                        .and_then(|v| v.parse().ok()),
                });
            }
            "audio" => {
                let channels = s.get("channels").and_then(|v| v.as_u64()).map(|v| v as u32);
                let is_eac3 = codec
                    .as_deref()
                    .map(|c| c.to_ascii_lowercase().starts_with("eac3"))
                    .unwrap_or(false);
                audio.push(AudioTrack {
                    index: aidx,
                    codec: codec.unwrap_or_default(),
                    language,
                    channels,
                    sample_rate: s
                        .get("sample_rate")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    atmos: is_eac3 && channels.map(|c| c >= 8).unwrap_or(false),
                });
                aidx += 1;
            }
            "subtitle" => {
                subtitles.push(SubtitleTrack {
                    index: sidx,
                    codec: codec.unwrap_or_default(),
                    language,
                    forced: s
                        .get("disposition")
                        .and_then(|d| d.get("forced"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        != 0,
                });
                sidx += 1;
            }
            _ => {}
        }
    }
    Ok(FileFacts {
        container,
        video,
        audio,
        subtitles,
        size,
        duration_s,
        bitrate_bps,
        marker,
        comment,
    })
}

fn codec_str(s: &Value) -> &str {
    s.get("codec_name").and_then(|v| v.as_str()).unwrap_or("")
}

/// Codec-aware level normalization (ffprobe `level` is `level_idc`).
///
/// H.264: `level_idc = major*10 + minor` (42 → 4.2).
/// HEVC: the idc is a lookup table (100→4.0, 110→4.1, 120→5.0,
/// 130→5.1, 140→5.2, 150→5.3, 153→6.0, 165→6.1) — NOT
/// major*10+minor.
fn norm_level(idc: u64, codec: &str) -> String {
    let codec = codec.to_ascii_lowercase();
    if codec == "hevc" || codec == "h265" {
        return match idc {
            100 => "4.0".into(),
            110 => "4.1".into(),
            120 => "5.0".into(),
            130 => "5.1".into(),
            140 => "5.2".into(),
            150 => "5.3".into(),
            153 => "6.0".into(),
            165 => "6.1".into(),
            _ => {
                let major = idc / 100;
                let minor = idc % 100;
                format!("{major}.{minor}")
            }
        };
    }
    if codec == "h264" || codec == "avc" {
        let major = idc / 10;
        let minor = idc % 10;
        return format!("{major}.{minor}");
    }
    idc.to_string()
}

/// Parse a rational frame rate string (`"24000/1001"`, `"25/1"`, `"0/0"`).
fn parse_rate(s: &str) -> Option<f64> {
    let (a, b) = s.split_once('/')?;
    let a: f64 = a.trim().parse().ok()?;
    let b: f64 = b.trim().parse().ok()?;
    if b == 0.0 || a == 0.0 {
        None
    } else {
        Some(a / b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(v: &str) -> Value {
        serde_json::from_str(v).unwrap()
    }

    #[test]
    fn maps_a_full_document() {
        let d = doc(r#"{
              "format": {"duration": "100.5", "size": "12345", "bit_rate": "98765"},
              "streams": [
                {"codec_type": "video", "codec_name": "hevc", "profile": "Main",
                 "level": 150, "pix_fmt": "yuv420p10le", "width": 3840, "height": 2160,
                 "avg_frame_rate": "25/1", "bit_rate": "98765",
                 "side_data_list": [{"side_data_type": "Dolby Vision"}]},
                {"codec_type": "audio", "codec_name": "eac3", "channels": 8,
                 "sample_rate": "48000", "tags": {"language": "eng"}},
                {"codec_type": "subtitle", "codec_name": "mov_text",
                 "tags": {"language": "eng"}, "disposition": {"forced": 1}}
              ]
            }"#);
        let facts = map_facts(Path::new("/media/show.s01e01.mkv"), &d, None).unwrap();
        assert_eq!(facts.container, "mkv");
        let v = facts.video.as_ref().unwrap();
        assert_eq!(&v.codec, "hevc");
        // HEVC level 150 → 5.3 (NOT 15.0 — that's the H.264 encoding).
        assert_eq!(v.level.as_deref(), Some("5.3"));
        assert_eq!(v.width, 3840);
        assert_eq!(v.frame_rate, Some(25.0));
        assert!(matches!(v.hdr, Hdr::DolbyVision));
        assert_eq!(facts.audio.len(), 1);
        assert!(facts.audio[0].atmos);
        assert_eq!(facts.audio[0].channels, Some(8));
        assert_eq!(facts.subtitles[0].index, 0);
        assert!(facts.subtitles[0].forced);
    }

    #[test]
    fn h264_level_encoding_differs_from_hevc() {
        let d = doc(r#"{"format": {"duration": "10"}, "streams": [
                {"codec_type": "video", "codec_name": "h264", "level": 42,
                 "pix_fmt": "yuv420p", "width": 1920, "height": 1080}
            ]}"#);
        let facts = map_facts(Path::new("/x.mp4"), &d, None).unwrap();
        assert_eq!(facts.video.as_ref().unwrap().level.as_deref(), Some("4.2"));
    }

    #[test]
    fn hdr10_is_mastering_display_plus_clli() {
        // A real HDR10 file carries both side data types. CLLI is
        // usually listed first — a naive "last one wins" chain would
        // misread this as HLG.
        let d = doc(r#"{"format": {"duration": "10"}, "streams": [
                {"codec_type": "video", "codec_name": "hevc",
                 "pix_fmt": "yuv420p10le", "width": 1920, "height": 1080,
                 "side_data_list": [
                   {"side_data_type": "Content light level"},
                   {"side_data_type": "Mastering display color volume"}
                 ]}
            ]}"#);
        let facts = map_facts(Path::new("/x.mkv"), &d, None).unwrap();
        assert!(matches!(facts.video.as_ref().unwrap().hdr, Hdr::Hdr10));
    }

    #[test]
    fn clli_alone_is_hlg() {
        let d = doc(r#"{"format": {"duration": "10"}, "streams": [
                {"codec_type": "video", "codec_name": "hevc",
                 "pix_fmt": "yuv420p10le", "width": 1920, "height": 1080,
                 "side_data_list": [{"side_data_type": "Content light level"}]}
            ]}"#);
        let facts = map_facts(Path::new("/x.mkv"), &d, None).unwrap();
        assert!(matches!(facts.video.as_ref().unwrap().hdr, Hdr::Hlg));
    }

    #[test]
    fn empty_document_is_safe() {
        let facts = map_facts(Path::new("/x.avi"), &doc("{}"), None).unwrap();
        assert_eq!(facts.container, "avi");
        assert!(facts.video.is_none());
        assert!(facts.audio.is_empty());
    }

    const MARKER: &str = "transcodarr:t1:5d1f076e9dbe36e27792d0199ccb7969";

    #[test]
    fn custom_tag_marker_is_read_case_insensitively() {
        // The MKV muxer uppercases the custom key; the reader must not
        // care.
        let d = doc(&format!(
            r#"{{"format": {{"duration": "10", "tags": {{"TRANSCODARR": "{}"}}}}, "streams": []}}"#,
            MARKER
        ));
        let facts = map_facts(Path::new("/x.mkv"), &d, None).unwrap();
        assert_eq!(facts.marker.as_deref(), Some(MARKER));
        assert_eq!(facts.comment, None);
    }

    #[test]
    fn comment_slot_marker_on_mp4() {
        // MP4 family: the marker rides the standard comment field.
        let d = doc(&format!(
            r#"{{"format": {{"duration": "10", "tags": {{"comment": "{}"}}}}, "streams": []}}"#,
            MARKER
        ));
        let facts = map_facts(Path::new("/x.mp4"), &d, None).unwrap();
        assert_eq!(facts.marker.as_deref(), Some(MARKER));
        assert_eq!(facts.comment.as_deref(), Some(MARKER));
    }

    #[test]
    fn user_comment_is_not_a_marker() {
        // A comment that does not match the marker grammar stays
        // user data: no marker, and the slot is reported as used (the
        // MP4/MOV marker must not clobber it, §6.6).
        let d = doc(
            r#"{"format": {"duration": "10", "tags": {"comment": "a note about life"}}, "streams": []}"#,
        );
        let facts = map_facts(Path::new("/x.mp4"), &d, None).unwrap();
        assert_eq!(facts.marker, None);
        assert_eq!(facts.comment.as_deref(), Some("a note about life"));
    }
}
