use serde::de;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::facts::{FileFacts, Hdr, VideoFacts};
use crate::plan::{FilterGraph, VideoEncode, VideoPlan};
use crate::registry::operation::parse;
use crate::registry::{OperationSection, SectionPlan};

/// The v1 video codec target, shared with the plan model.
pub use crate::plan::VideoTargetCodec as VideoCodec;

/// `video` — the video-domain policy for a matched file (DESIGN §6.1).
///
/// Absent section ⇒ stream copy. Present section ⇒ an explicit target:
/// codec, profile, level, bitrate mode, device, optional downscale
/// (never upscale), explicit HDR→SDR flag.
///
/// Flow JSON:
/// ```json
/// { "video": {
///   "container": "smart",
///   "codec": "h264",
///   "profile": "auto",
///   "level": "auto",
///   "bitrate": { "mode": "source_capped" },
///   "device": "auto",
///   "downscale_to": [1920, 1080],
///   "hdr_to_sdr": false,
///   "video_filter": "crop=1920:800:0:0"
/// } }
/// ```
/// 1080p reference pixel count for bitrate-ceiling scaling.
const REF_1080P_PIXELS: u64 = 1920 * 1080;

/// Source-capped ceiling for a codec at a target resolution
/// (DESIGN §6.1: "≤ source bitrate, ceiling scaled by target
/// resolution").
#[must_use]
pub fn source_capped_ceiling(codec: VideoCodec, target_pixels: u64) -> u64 {
    let base_1080p: u64 = match codec {
        VideoCodec::H264 => 12_000_000,
        VideoCodec::Hevc => 6_000_000,
    };
    base_1080p.saturating_mul(target_pixels) / REF_1080P_PIXELS
}

/// Target container choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerChoice {
    /// MP4 if every planned stream is MP4-safe, else MKV (DESIGN §13.7).
    #[default]
    Smart,
    Mp4,
    Mkv,
}

/// Profile: `auto` (sensible default per codec + bit depth) or an
/// explicit ffmpeg profile name.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ProfileSpec {
    /// Codec default (H.264: `high`/`high10`; HEVC: `main`/`main10`).
    #[default]
    Auto,
    Explicit(String),
}

/// Level: `auto` (sensible default per codec + resolution) or an
/// explicit level like `"4.2"`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LevelSpec {
    /// Codec/resolution default (e.g. 4K HEVC → `5.1`, DESIGN §6.1).
    #[default]
    Auto,
    Explicit(String),
}

impl Serialize for ProfileSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Explicit(p) => s.serialize_str(p),
        }
    }
}

impl<'de> Deserialize<'de> for ProfileSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Bare `auto` means codec default; any other string is explicit.
        // `null` is accepted as `auto`: the earlier untagged serde
        // serialized Auto as null, and pre-existing flows keep it.
        let v = Value::deserialize(d)?;
        match v {
            Value::Null => Ok(Self::Auto),
            Value::String(s) if s.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Value::String(s) => Ok(Self::Explicit(s)),
            other => Err(de::Error::custom(format!("invalid profile spec: {other}"))),
        }
    }
}

impl Serialize for LevelSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Explicit(l) => s.serialize_str(l),
        }
    }
}

impl<'de> Deserialize<'de> for LevelSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // `null` is accepted as `auto` (the earlier untagged serde
        // serialized Auto as null; pre-existing flows keep it).
        let v = Value::deserialize(d)?;
        match v {
            Value::Null => Ok(Self::Auto),
            Value::String(s) if s.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Value::String(s) => Ok(Self::Explicit(s)),
            other => Err(de::Error::custom(format!("invalid level spec: {other}"))),
        }
    }
}

/// Bitrate mode (DESIGN §6.1).
///
/// JSON: `{ "mode": "source_capped" | "fixed", "bps": … }` or
/// `{ "mode": "crf", "value": … }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BitrateMode {
    /// Default: ≤ source bitrate, ceiling scaled by target resolution.
    #[default]
    SourceCapped,
    /// Fixed bitrate in bits/s.
    Fixed { bps: u64 },
    /// Quality-based (CRF).
    Crf { value: u32 },
}

/// Encode device choice (populated from the startup encoder probe;
/// unavailable encoders are shown disabled in the UI — DESIGN §6.1).
///
/// JSON: `"auto"` | `"software"` | `{ "kind": "gpu", "id": "…" }`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DeviceChoice {
    /// Best available device at dispatch time (GPU first, CPU fallback).
    #[default]
    Auto,
    /// Always the CPU pseudo-device.
    Software,
    /// A specific detected device by id.
    Gpu { id: String },
}

impl Serialize for DeviceChoice {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => ser.serialize_str("auto"),
            Self::Software => ser.serialize_str("software"),
            Self::Gpu { id } => {
                let mut s = ser.serialize_struct("Gpu", 2)?;
                s.serialize_field("kind", "gpu")?;
                s.serialize_field("id", id)?;
                s.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for DeviceChoice {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        // Unit variants cannot sit inside an untagged enum (they only match
        // null, making Auto and Software indistinguishable), so the three
        // documented JSON forms are matched explicitly.
        let v = Value::deserialize(des)?;
        match v {
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "auto" => Ok(Self::Auto),
                "software" | "cpu" => Ok(Self::Software),
                other => Err(de::Error::unknown_variant(other, &["auto", "software"])),
            },
            Value::Object(m) => {
                let id = m.get("id").and_then(Value::as_str);
                let kind = m
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase);
                match (kind, id) {
                    // {"kind":"gpu","id":…} — canonical form.
                    (Some(k), Some(id)) if k == "gpu" => Ok(Self::Gpu { id: id.into() }),
                    // {"id":…} — how the old untagged derive serialized
                    // Gpu (no kind tag); accepted so pre-existing flows
                    // upgrade without a manual edit.
                    (None, Some(id)) => Ok(Self::Gpu { id: id.into() }),
                    _ => Err(de::Error::custom(
                        "gpu device choice needs {\"kind\":\"gpu\",\"id\":\"…\"}".to_owned(),
                    )),
                }
            }
            other => Err(de::Error::custom(format!("invalid device choice: {other}"))),
        }
    }
}

/// The `video` section parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoOp {
    #[serde(default)]
    pub container: ContainerChoice,
    pub codec: VideoCodec,
    #[serde(default)]
    pub profile: ProfileSpec,
    #[serde(default)]
    pub level: LevelSpec,
    #[serde(default)]
    pub bitrate: BitrateMode,
    #[serde(default)]
    pub device: DeviceChoice,
    /// Optional downscale target `[width, height]`. Never upscales:
    /// sources smaller than the target keep their resolution.
    #[serde(default)]
    pub downscale_to: Option<[u32; 2]>,
    /// Explicit HDR→SDR conversion. Default off — destructive to look,
    /// never implicit (DESIGN §6.1).
    #[serde(default)]
    pub hdr_to_sdr: bool,
    /// User filter graph applied to the video stream (DESIGN §6.5).
    /// A non-empty graph always re-encodes (filters cannot be applied
    /// to a stream being copied).
    #[serde(default, skip_serializing_if = "FilterGraph::is_empty")]
    pub video_filter: FilterGraph,
}

/// Default profile per codec + bit depth.
#[must_use]
pub fn default_profile(codec: VideoCodec, is_10bit: bool) -> &'static str {
    match (codec, is_10bit) {
        (VideoCodec::H264, false) => "high",
        (VideoCodec::H264, true) => "high10",
        (VideoCodec::Hevc, false) => "main",
        (VideoCodec::Hevc, true) => "main10",
    }
}

/// Default level per codec + target resolution (DESIGN §6.1 examples:
/// 4K HEVC → 5.1; 1080p H.264 → 4.2).
#[must_use]
pub fn default_level(codec: VideoCodec, target_pixels: u64) -> &'static str {
    let p = target_pixels;
    let l1080 = REF_1080P_PIXELS;
    let l1440 = 2560 * 1440;
    let l4k = 3840 * 2160;
    match codec {
        VideoCodec::H264 => {
            if p <= 1280 * 720 {
                "4.0"
            } else if p <= l1080 {
                "4.2"
            } else if p <= l1440 {
                "5.0"
            } else if p <= l4k {
                "5.1"
            } else {
                "5.2"
            }
        }
        VideoCodec::Hevc => {
            if p <= 1280 * 720 {
                "3.1"
            } else if p <= l1080 {
                "4.0"
            } else if p <= l1440 {
                "4.1"
            } else if p <= l4k {
                "5.1"
            } else {
                "5.2"
            }
        }
    }
}

/// Normalize a level for comparison (`"4.2"` ≡ `"42"`).
fn norm_level(level: &str) -> u64 {
    level.trim().replace('.', "").parse().unwrap_or(0)
}

/// Would this operation change anything for this file's video stream?
///
/// The compliance definition (DESIGN §2): identity ⇔ same codec, no
/// downscale, no HDR→SDR conversion, profile/level match (or `auto`),
/// and the source-capped ceiling not exceeded. `crf`/`fixed` modes
/// always re-encode.
#[must_use]
pub fn is_identity(op: &VideoOp, v: &VideoFacts) -> bool {
    // A non-empty filter graph is always a change (it forces an
    // encode; there is no such thing as a filtered copy).
    if !op.video_filter.is_empty() {
        return false;
    }
    // Codec must already be the target.
    if !v.codec.eq_ignore_ascii_case(op.codec.name()) {
        return false;
    }
    // A real downscale is a change. (Targets ≥ source are no-ops:
    // we never upscale.)
    if let Some([tw, th]) = op.downscale_to {
        if (tw as u64) * (th as u64) < v.pixels() {
            return false;
        }
    }
    // Explicit HDR→SDR on an HDR source is a change.
    if op.hdr_to_sdr && v.hdr != Hdr::None {
        return false;
    }
    // Explicit profile/level must match the source (else re-encode).
    match &op.profile {
        ProfileSpec::Auto => {}
        ProfileSpec::Explicit(p) => {
            if !v
                .profile
                .as_deref()
                .is_some_and(|src| src.eq_ignore_ascii_case(p))
            {
                return false;
            }
        }
    }
    match &op.level {
        LevelSpec::Auto => {}
        LevelSpec::Explicit(l) => {
            if !v
                .level
                .as_deref()
                .is_some_and(|src| norm_level(src) == norm_level(l))
            {
                return false;
            }
        }
    }
    // crf / fixed always re-encode; source-capped re-encodes only when
    // the source exceeds the (resolution-scaled) ceiling.
    match op.bitrate {
        BitrateMode::Crf { .. } | BitrateMode::Fixed { .. } => false,
        BitrateMode::SourceCapped => {
            let target_pixels = op
                .downscale_to
                .map_or(v.pixels(), |t| (t[0] as u64) * (t[1] as u64));
            let target_pixels = target_pixels.min(v.pixels()); // never upscale
            if let Some(src_bps) = v.bitrate_bps {
                src_bps <= source_capped_ceiling(op.codec, target_pixels)
            } else {
                true // unknown bitrate → assume compliant (conservative: don't re-encode blindly)
            }
        }
    }
}

/// Build the filter chain for a real video encode.
fn build_filters(v: &VideoFacts, op: &VideoOp, target_w: u32, target_h: u32) -> Vec<String> {
    let mut filters = Vec::new();
    let downscales = (target_w, target_h) != (v.width, v.height);
    if downscales {
        filters.push(format!("scale={target_w}:{target_h}:flags=lanczos"));
    }
    // HDR→SDR. Only applied when the source is actually HDR (the
    // design: "no-op when source is SDR"). The exact chain is an
    // implementation detail (DESIGN §6.1).
    if op.hdr_to_sdr && v.hdr != Hdr::None {
        let out_fmt = if v.is_10bit() {
            "yuv420p10le"
        } else {
            "yuv420p"
        };
        filters.push("zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709".to_string());
        filters.push("tonemap=tonemap=bt.2390:desat=0".to_string());
        filters.push(format!("zscale=t=bt709:m=bt709:r=tv,format={out_fmt}"));
    }
    // The user's graph comes last: after downscale and HDR conversion
    // (DESIGN §6.5).
    if !op.video_filter.is_empty() {
        filters.push(op.video_filter.0.clone());
    }
    filters
}

/// The `video` section entry.
pub struct Video;

/// Build a [`VideoPlan`] for a non-identity operation. The encoder is
/// resolved to the CPU default here; [`to_argv`](crate::plan) re-resolves
/// it from the device that will run the job.
fn build_plan(op: &VideoOp, v: &VideoFacts) -> VideoPlan {
    // Target resolution: never upscale.
    let mut target_w = v.width;
    let mut target_h = v.height;
    if let Some([tw, th]) = op.downscale_to {
        if (tw as u64) * (th as u64) < v.pixels() {
            target_w = tw;
            target_h = th;
        }
    }
    let target_pixels = (target_w as u64) * (target_h as u64);

    // Profile / level.
    let profile = match &op.profile {
        ProfileSpec::Auto => default_profile(op.codec, v.is_10bit()).to_string(),
        ProfileSpec::Explicit(p) => p.clone(),
    };
    let level = match &op.level {
        LevelSpec::Auto => default_level(op.codec, target_pixels).to_string(),
        LevelSpec::Explicit(l) => l.clone(),
    };

    // Bitrate.
    let (bitrate, maxrate, bufsize, crf) = match op.bitrate {
        BitrateMode::SourceCapped => {
            let ceiling = source_capped_ceiling(op.codec, target_pixels);
            let bps = v.bitrate_bps.map(|s| s.min(ceiling)).unwrap_or(ceiling);
            (Some(bps), Some(bps * 115 / 100), Some(bps * 2), None)
        }
        BitrateMode::Fixed { bps } => (Some(bps), Some(bps * 115 / 100), Some(bps * 2), None),
        BitrateMode::Crf { value } => (None, None, None, Some(value)),
    };

    let pix_fmt = if v.is_10bit() {
        "yuv420p10le"
    } else {
        "yuv420p"
    }
    .to_string();

    VideoPlan::Encode(Box::new(VideoEncode {
        codec: op.codec,
        encoder: op.codec.cpu_encoder().to_string(),
        profile,
        level,
        bitrate_bps: bitrate,
        maxrate_bps: maxrate,
        bufsize_bps: bufsize,
        crf,
        pix_fmt,
        filters: build_filters(v, op, target_w, target_h),
        filter: if op.video_filter.is_empty() {
            None
        } else {
            Some(op.video_filter.clone())
        },
        target_width: target_w,
        target_height: target_h,
    }))
}

impl OperationSection for Video {
    fn key(&self) -> &'static str {
        "video"
    }

    fn description(&self) -> &'static str {
        "Video: target codec/profile/level, bitrate mode, device, downscale, HDR→SDR"
    }

    fn plan(&self, params: &Value, facts: &FileFacts) -> crate::error::Result<SectionPlan> {
        let op: VideoOp = parse(self.key(), params)?;
        // A video section on a file without a video stream changes nothing.
        let Some(v) = facts.video() else {
            return Ok(SectionPlan::Identity);
        };
        if is_identity(&op, v) {
            return Ok(SectionPlan::Identity);
        }
        Ok(SectionPlan::Video(build_plan(&op, v)))
    }

    fn validate(&self, params: &Value) -> crate::error::Result<()> {
        parse::<VideoOp>(self.key(), params).map(|_| ())
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "object",
            "label": "Video",
            "fields": {
                "container": {
                    "kind": "single_select",
                    "label": "Container",
                    "values": [
                        { "value": "smart", "label": "Auto (MP4 when safe, else MKV)" },
                        { "value": "mp4", "label": "MP4" },
                        { "value": "mkv", "label": "MKV" }
                    ],
                    "default": "smart",
                    "hint": "Auto picks MP4 when the output is MP4-safe, otherwise MKV."
                },
                "codec": {
                    "kind": "single_select",
                    "label": "Video codec",
                    "values": [
                        { "value": "h264", "label": "H.264" },
                        { "value": "hevc", "label": "HEVC" }
                    ],
                    "default": "h264",
                    "hint": "AV1 is not available yet."
                },
                "profile": { "kind": "text", "label": "Profile", "default": "auto", "hint": "Leave as auto for the codec default at the source bit depth." },
                "level": { "kind": "text", "label": "Level", "default": "auto", "hint": "Leave as auto for the codec default at the target resolution." },
                "bitrate": {
                    "kind": "bitrate_mode", "label": "Bitrate", "values": [{ "value": "source_capped", "label": "Source-capped" }, { "value": "fixed", "label": "Fixed bitrate" }, { "value": "crf", "label": "CRF" }],
                    "default": "source_capped",
                    "hint": "Source-capped: at or below the source bitrate, scaled to the target resolution."
                },
                "device": {
                    "kind": "device_select",
                    "label": "Device",
                    "default": "auto",
                    "hint": "Chosen from the encoders available on this machine."
                },
                "downscale_to": {
                    "kind": "resolution",
                    "label": "Downscale to",
                    "hint": "Never upscales. Sources at or above the target keep their resolution."
                },
                "hdr_to_sdr": { "kind": "boolean", "label": "Convert HDR to SDR", "default": false, "hint": "Changes the look of the image. Only enabled when you ask." },
                "video_filter": { "kind": "text", "label": "Video filter", "hint": "An ffmpeg -vf expression (e.g. crop=1920:800:0:0,denoise). Applied after everything else; one-shot — runs once per file." }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_level_auto_string_round_trips() {
        let p: ProfileSpec = serde_json::from_str("\"auto\"").unwrap();
        assert!(matches!(p, ProfileSpec::Auto));
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"auto\"");
        let p: ProfileSpec = serde_json::from_str("\"high\"").unwrap();
        assert!(matches!(p, ProfileSpec::Explicit(ref x) if x == "high"));
        let l: LevelSpec = serde_json::from_str("\"auto\"").unwrap();
        assert!(matches!(l, LevelSpec::Auto));
        let l: LevelSpec = serde_json::from_str("\"4.2\"").unwrap();
        assert!(matches!(l, LevelSpec::Explicit(ref x) if x == "4.2"));
        // Legacy: the old untagged derive serialized Auto as null.
        let p: ProfileSpec = serde_json::from_str("null").unwrap();
        assert!(matches!(p, ProfileSpec::Auto));
        let l: LevelSpec = serde_json::from_str("null").unwrap();
        assert!(matches!(l, LevelSpec::Auto));
        // Round-trip now normalizes null back to the string form.
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"auto\"");
    }

    fn video(codec: &str, w: u32, h: u32, bps: Option<u64>) -> VideoFacts {
        VideoFacts {
            codec: codec.into(),
            profile: None,
            level: None,
            pixel_format: Some("yuv420p".into()),
            width: w,
            height: h,
            hdr: Hdr::None,
            bitrate_bps: bps,
            ..Default::default()
        }
    }

    fn facts(v: Option<VideoFacts>) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            video: v,
            ..Default::default()
        }
    }

    /// Unwrap a test's plan into its Encode plan (panics otherwise).
    fn enc(p: SectionPlan) -> VideoPlan {
        match p {
            SectionPlan::Video(v) => v,
            other => panic!("expected a Video plan, got {other:?}"),
        }
    }

    #[test]
    fn identical_file_is_identity() {
        let s = Video;
        // 1080p h264, well under the source-capped ceiling → identity.
        let f = facts(Some(video("h264", 1920, 1080, Some(8_000_000))));
        let op: VideoOp = serde_json::from_value(
            json!({ "codec": "h264", "bitrate": { "mode": "source_capped" } }),
        )
        .unwrap();
        assert!(is_identity(&op, f.video().unwrap()));
        assert!(matches!(
            s.plan(&json!({ "codec": "h264" }), &f).unwrap(),
            SectionPlan::Identity
        ));
    }

    #[test]
    fn codec_change_is_not_identity() {
        let s = Video;
        let f = facts(Some(video("hevc", 1920, 1080, Some(6_000_000))));
        let p = s.plan(&json!({ "codec": "h264" }), &f).unwrap();
        assert!(!matches!(p, SectionPlan::Identity));
        let VideoPlan::Encode(e) = enc(p) else {
            panic!("expected Encode")
        };
        assert_eq!(e.codec, VideoCodec::H264);
    }

    #[test]
    fn downscale_plans_scale_filter() {
        let s = Video;
        // 4K HEVC 80 Mb/s → 1080p H.264 (the design's worked example).
        let f = facts(Some(video("hevc", 3840, 2160, Some(80_000_000))));
        let j = json!({
            "codec": "h264",
            "downscale_to": [1920, 1080],
            "bitrate": { "mode": "source_capped" }
        });
        let VideoPlan::Encode(e) = enc(s.plan(&j, &f).unwrap()) else {
            panic!("expected Encode")
        };
        assert_eq!((e.target_width, e.target_height), (1920, 1080));
        assert!(e.filters.iter().any(|f| f.starts_with("scale=1920:1080")));
        // source-capped: ceiling at 1080p = 12 Mb/s → below the 80 Mb/s source.
        assert_eq!(e.bitrate_bps, Some(12_000_000));
        assert_eq!(e.maxrate_bps, Some(13_800_000));
        assert_eq!(e.bufsize_bps, Some(24_000_000));
    }

    #[test]
    fn never_upscales() {
        let s = Video;
        let f = facts(Some(video("h264", 1280, 720, Some(5_000_000))));
        let j = json!({ "codec": "h264", "downscale_to": [1920, 1080], "bitrate": { "mode": "crf", "value": 20 } });
        let VideoPlan::Encode(e) = enc(s.plan(&j, &f).unwrap()) else {
            panic!("expected Encode")
        };
        // 720p source < 1080p target → stays 720p (CRF still re-encodes).
        assert_eq!((e.target_width, e.target_height), (1280, 720));
        assert_eq!(e.crf, Some(20));
        assert!(e.bitrate_bps.is_none());
    }

    #[test]
    fn hdr_to_sdr_adds_tonemap_chain_only_for_hdr() {
        let s = Video;
        let mut v = video("hevc", 3840, 2160, Some(60_000_000));
        v.hdr = Hdr::Hdr10;
        let f = facts(Some(v));
        let j = json!({ "codec": "h264", "hdr_to_sdr": true });
        let VideoPlan::Encode(e) = enc(s.plan(&j, &f).unwrap()) else {
            panic!("expected Encode")
        };
        assert!(e.filters.iter().any(|f| f.contains("tonemap")));

        // SDR source → no-op, no filters.
        let f = facts(Some(video("h264", 1920, 1080, Some(8_000_000))));
        let j = json!({ "codec": "h264", "hdr_to_sdr": true, "profile": "high", "level": "4.2" });
        let VideoPlan::Encode(e) = enc(s.plan(&j, &f).unwrap()) else {
            panic!("expected Encode")
        };
        assert!(
            !e.filters.iter().any(|f| f.contains("tonemap")),
            "HDR→SDR must be a no-op on an SDR source"
        );
    }

    #[test]
    fn source_cap_fires_reencode() {
        // 1080p h264 at 40 Mb/s exceeds the 12 Mb/s ceiling → re-encode.
        let s = Video;
        let f = facts(Some(video("h264", 1920, 1080, Some(40_000_000))));
        let p = s
            .plan(
                &json!({ "codec": "h264", "bitrate": { "mode": "source_capped" } }),
                &f,
            )
            .unwrap();
        assert!(!matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn profile_level_defaults() {
        assert_eq!(default_profile(VideoCodec::H264, false), "high");
        assert_eq!(default_profile(VideoCodec::H264, true), "high10");
        assert_eq!(default_profile(VideoCodec::Hevc, true), "main10");
        assert_eq!(default_level(VideoCodec::Hevc, 3840 * 2160), "5.1");
        assert_eq!(default_level(VideoCodec::H264, 1920 * 1080), "4.2");
    }

    #[test]
    fn explicit_profile_mismatch_reencodes() {
        let s = Video;
        let mut v = video("h264", 1920, 1080, Some(8_000_000));
        v.profile = Some("baseline".into());
        let f = facts(Some(v));
        let p = s
            .plan(&json!({ "codec": "h264", "profile": "high" }), &f)
            .unwrap();
        assert!(!matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn level_compare_ignores_dot() {
        let s = Video;
        let mut v = video("h264", 1920, 1080, Some(8_000_000));
        v.level = Some("42".into()); // ffprobe style
        let f = facts(Some(v));
        let p = s
            .plan(&json!({ "codec": "h264", "level": "4.2" }), &f)
            .unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn device_choice_json_forms_round_trip() {
        // Regression: untagged unit variants only match null, so the
        // documented string forms used to fail to deserialize.
        assert_eq!(
            DeviceChoice::deserialize(&Value::String("auto".into())).unwrap(),
            DeviceChoice::Auto
        );
        assert_eq!(
            DeviceChoice::deserialize(&Value::String("software".into())).unwrap(),
            DeviceChoice::Software
        );
        assert_eq!(
            DeviceChoice::deserialize(&json!({ "kind": "gpu", "id": "nvenc-0" })).unwrap(),
            DeviceChoice::Gpu {
                id: "nvenc-0".into()
            }
        );
        assert_eq!(
            serde_json::to_string(&DeviceChoice::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&DeviceChoice::Software).unwrap(),
            "\"software\""
        );
        assert_eq!(
            serde_json::to_string(&DeviceChoice::Gpu { id: "x".into() }).unwrap(),
            "{\"kind\":\"gpu\",\"id\":\"x\"}"
        );
        assert!(DeviceChoice::deserialize(&Value::String("nvidia".into())).is_err());
        // Legacy bare {"id":…} (old untagged derive) still parses as Gpu.
        assert_eq!(
            DeviceChoice::deserialize(&json!({ "id": "x" })).unwrap(),
            DeviceChoice::Gpu { id: "x".into() }
        );
        // A gpu object without the id (or with a foreign kind) is rejected.
        assert!(DeviceChoice::deserialize(&json!({ "kind": "gpu" })).is_err());
        assert!(DeviceChoice::deserialize(&json!({ "kind": "nvidia", "id": "x" })).is_err());
    }
    #[test]
    fn filter_graph_validation() {
        // Trimmed, accepted.
        let g: FilterGraph = serde_json::from_str("\"  denoise  \"").unwrap();
        assert_eq!(g.0, "denoise");
        assert!(!g.is_empty());
        // Empty / absent is the default (no filter).
        let g: FilterGraph = serde_json::from_str("\"\"").unwrap();
        assert!(g.is_empty());
        assert_eq!(
            serde_json::to_string(&FilterGraph::default()).unwrap(),
            "\"\""
        );
        // Control characters are rejected (a graph must stay one line
        // of argv, one token of filter grammar).
        assert!(serde_json::from_str::<FilterGraph>("\"a\nb\"").is_err());
        // Length is bounded.
        let long = "\"a\"".repeat(4097);
        assert!(serde_json::from_str::<FilterGraph>(&long).is_err());
    }

    #[test]
    fn filter_forces_encode_and_composes_after_scale() {
        let s = Video;
        // Same codec, well under the cap — identity without a filter.
        let f = facts(Some(video("h264", 1920, 1080, Some(8_000_000))));
        assert!(matches!(
            s.plan(&json!({ "codec": "h264", "video_filter": "" }), &f)
                .unwrap(),
            SectionPlan::Identity
        ));
        // A non-empty filter is a change, even when the codec matches.
        let p = s
            .plan(&json!({ "codec": "h264", "video_filter": "denoise" }), &f)
            .unwrap();
        let VideoPlan::Encode(e) = enc(p) else {
            panic!("expected Encode")
        };
        assert_eq!(e.filters, vec!["denoise".to_string()]);
        assert_eq!(e.filter, Some(FilterGraph("denoise".into())));

        // The user graph composes LAST: after a real downscale.
        let f = facts(Some(video("hevc", 3840, 2160, Some(80_000_000))));
        let j = json!({
            "codec": "h264",
            "downscale_to": [1920, 1080],
            "video_filter": "crop=1920:800:0:0"
        });
        let VideoPlan::Encode(e) = enc(s.plan(&j, &f).unwrap()) else {
            panic!("expected Encode")
        };
        assert_eq!(
            e.filters,
            vec![
                "scale=1920:1080:flags=lanczos".to_string(),
                "crop=1920:800:0:0".to_string()
            ]
        );
    }

    #[test]
    fn validate_rejects_oversized_filter() {
        let j = json!({ "codec": "h264", "video_filter": "a".repeat(4097) });
        assert!(matches!(
            Video.validate(&j),
            Err(crate::error::CoreError::Flow(_))
        ));
    }

    #[test]
    fn validate_accepts_typed_params() {
        Video
            .validate(&json!({ "codec": "h264", "video_filter": "denoise" }))
            .unwrap();
    }

    // (module closes)
}
