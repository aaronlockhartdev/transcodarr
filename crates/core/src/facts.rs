use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub sample_rate: Option<u32>,
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
