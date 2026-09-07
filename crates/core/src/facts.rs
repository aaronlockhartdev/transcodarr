use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HDR metadata detected on a video stream.
///
/// Values match what a fact extractor can determine from probe metadata
/// (`hdr10`, `hdr10+`, `dolby-vision`, `hlg`); `None` means SDR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hdr {
    /// No HDR metadata (SDR).
    #[default]
    None,
    Hdr10,
    Hdr10Plus,
    /// Dolby Vision (v1 does not distinguish versions/profiles).
    DolbyVision,
    Hlg,
}

/// Probed properties of the file's (single) video stream.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoFacts {
    /// Codec name as reported by the probe (`"hevc"`, `"h264"`, …).
    pub codec: String,
    #[serde(default)]
    pub profile: Option<String>,
    /// Normalized level string, e.g. `"4.2"`.
    #[serde(default)]
    pub level: Option<String>,
    /// Pixel format, e.g. `"yuv420p"`, `"yuv420p10le"`.
    #[serde(default)]
    pub pixel_format: Option<String>,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub frame_rate: Option<f64>,
    #[serde(default)]
    pub hdr: Hdr,
    /// Video stream bitrate in bits/s, if the probe reports one.
    #[serde(default)]
    pub bitrate_bps: Option<u64>,
}

impl VideoFacts {
    /// Pixel count (for resolution comparisons, DESIGN §5).
    #[must_use]
    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Whether the source is 10-bit (drives profile/pixel-format defaults).
    #[must_use]
    pub fn is_10bit(&self) -> bool {
        self.pixel_format
            .as_deref()
            .is_some_and(|p| p.contains("10") || p.contains("12") || p.contains("14"))
    }
}

/// One audio track of the file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Zero-based index of the audio stream in the file.
    pub index: u32,
    /// Codec name (`"eac3"`, `"dts"`, `"truehd"`, …).
    pub codec: String,
    #[serde(default)]
    pub language: Option<String>,
    /// Channel count (channel layout) when known.
    #[serde(default)]
    pub channels: Option<u32>,
    /// Sample rate in Hz, when known.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Stream title (ffprobe `tags.title`), if the container carries
    /// one — the match target for the audio rule's `title_contains`.
    #[serde(default)]
    pub title: Option<String>,
    /// The file's default audio track (ffprobe `disposition.default`).
    #[serde(default)]
    pub default: bool,
    /// Dolby Atmos (E-AC-3 JOC). Never auto-downmixed (DESIGN §6.2).
    #[serde(default)]
    pub atmos: bool,
}

/// One subtitle track of the file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    /// Zero-based index of the subtitle stream in the file.
    pub index: u32,
    /// Codec name (`"srt"`, `"ass"`, `"webvtt"`, `"hdmv_pgs_subtitle"`, …).
    pub codec: String,
    #[serde(default)]
    pub language: Option<String>,
    /// Forced flag (e.g. for SDH / forced translations).
    #[serde(default)]
    pub forced: bool,
}

/// Cached probe result for one file (DESIGN §2, §4).
///
/// This is the unit of evaluation: `evaluate()` is a pure function of
/// `(Flow, FileFacts)`, so re-evaluating after a flow edit never
/// re-probes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    /// Container/format name as reported by the probe (`"mkv"`, `"mp4"`, …).
    pub container: String,
    #[serde(default)]
    pub video: Option<VideoFacts>,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
    /// File size in bytes.
    pub size: u64,
    /// Duration in seconds (0.0 when unknown).
    #[serde(default)]
    pub duration_s: f64,
    /// Overall file bitrate in bits/s, if known.
    #[serde(default)]
    pub bitrate_bps: Option<u64>,
    /// The in-file processed marker carried in a format tag (§6.6):
    /// `transcodarr:t<ver>:<32-hex>`, or the comment slot's contents on
    /// MP4/MOV. `None` when the file carries no marker.
    #[serde(default)]
    pub marker: Option<String>,
    /// The format-level `comment` tag, if the source carries one (MP4/MOV
    /// slot-usage check for the marker, §6.6).
    #[serde(default)]
    pub comment: Option<String>,
}

impl FileFacts {
    /// Video stream facts, if the file has a video stream.
    #[must_use]
    pub fn video(&self) -> Option<&VideoFacts> {
        self.video.as_ref()
    }

    /// The video bitrate, falling back to the whole-file bitrate.
    #[must_use]
    pub fn video_bitrate_bps(&self) -> Option<u64> {
        self.video
            .as_ref()
            .and_then(|v| v.bitrate_bps)
            .or(self.bitrate_bps)
    }
}

/// The identity and content fingerprint of one file at one moment in
/// time (DESIGN §4, §6.6).
///
/// dev+inode+size+mtime is the primary change signal (a new file on a
/// new volume, a hardlink re-point, a replacement file all move at
/// least one of these); `sample_hash` (xxh3-128 over the three
/// 256 KB windows, §4) is the secondary signal that catches
/// same-size in-place rewrites of the sampled regions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub dev: i64,
    pub inode: i64,
    /// Size in bytes.
    pub size: i64,
    /// Unix mtime (seconds).
    pub mtime: i64,
    /// xxh3-128 hex over the sampled windows (§4, §13.15).
    pub sample_hash: String,
}

/// The per-file **applied-operations record** — the `files.applied_ops`
/// JSON (DESIGN §6.6): the full applied operation plus the file
/// fingerprint at the moment of tagging.
///
/// In-place, this record and the in-file marker are redundant memories
/// of the same fact: the marker survives a lost database, the record
/// survives a metadata-stripping tool. `marker` is `None` when the
/// marker stayed DB-only (disabled, or the MP4/MOV comment slot was in
/// use) or for records written before markers existed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedOps {
    /// The in-file marker written with this operation (`None` =
    /// DB-only, see the type doc).
    #[serde(default)]
    pub marker: Option<String>,
    /// The matched step's authored operation as written in the flow
    /// (the fingerprint input, §6.6).
    #[serde(default)]
    pub ops_json: Value,
    /// The file's fingerprint at the moment the operation was applied.
    pub file: FileFingerprint,
    /// The job that wrote this record (audit only).
    #[serde(default)]
    pub job_id: Option<String>,
    /// When the operation was applied, unix seconds (audit only).
    #[serde(default)]
    pub applied_at: Option<i64>,
}
