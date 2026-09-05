use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::device::{Device, hwaccel_for};

/// Target video codecs for re-encode (DESIGN §6.1).
///
/// H.264 and HEVC in v1; AV1 is parked (DESIGN §12) — adding it is one
/// variant + encoder entries, no migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoTargetCodec {
    H264,
    Hevc,
}

impl VideoTargetCodec {
    /// Codec name as it appears in probe output / ffmpeg (`"h264"`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
        }
    }

    /// Parse a raw codec name from probe output.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "h264" | "avc" => Some(Self::H264),
            "hevc" | "h265" => Some(Self::Hevc),
            _ => None,
        }
    }

    /// CPU (software) encoder name.
    #[must_use]
    pub fn cpu_encoder(&self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::Hevc => "libx265",
        }
    }
}

/// Target audio codecs for re-encode (DESIGN §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioTargetCodec {
    /// E-AC-3 (also the Atmos-preserving target).
    Eac3,
    /// AC-3.
    Ac3,
    /// AAC.
    Aac,
}

impl AudioTargetCodec {
    /// Codec name (`"eac3"`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Eac3 => "eac3",
            Self::Ac3 => "ac3",
            Self::Aac => "aac",
        }
    }

    /// ffmpeg encoder name.
    #[must_use]
    pub fn encoder(&self) -> &'static str {
        match self {
            Self::Eac3 => "libeac3",
            Self::Ac3 => "libac3",
            Self::Aac => "aac",
        }
    }

    /// Default bitrate for a re-encode (bits/s), by channel count.
    #[must_use]
    pub fn default_bitrate_bps(&self, channels: Option<u32>) -> u64 {
        let ch = channels.unwrap_or(2);
        match self {
            Self::Eac3 => match ch {
                1 => 160_000,
                2 => 320_000,
                _ => 640_000, // 5.1 and up
            },
            Self::Ac3 => match ch {
                1 => 96_000,
                2 => 192_000,
                _ => 448_000,
            },
            Self::Aac => match ch {
                1 => 64_000,
                2 => 128_000,
                _ => 320_000,
            },
        }
    }
}

/// The video-domain decision for one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VideoPlan {
    /// Stream-copy the existing video (the default for everything).
    Copy,
    /// Re-encode with an explicit target.
    Encode {
        /// Target codec (drives encoder selection on the device).
        codec: VideoTargetCodec,
        /// Encoder to use; re-resolved from the device at dispatch
        /// (a `h264_nvenc` on a GPU, `libx264` on CPU).
        encoder: String,
        /// Target profile (resolved from `auto` at planning time).
        profile: String,
        /// Target level, e.g. `"5.1"` (resolved from `auto`).
        level: String,
        /// Output video bitrate in bits/s (bitrate/fixed modes).
        bitrate_bps: Option<u64>,
        /// Peak limiter (1.15× bitrate).
        maxrate_bps: Option<u64>,
        /// VBV buffer (2× bitrate).
        bufsize_bps: Option<u64>,
        /// CRF (CRF mode only).
        crf: Option<u32>,
        /// Output pixel format (matches source bit depth).
        pix_fmt: String,
        /// `-vf` chain: downscale filter, HDR→SDR conversion, …
        filters: Vec<String>,
        /// Output resolution (never upscaled).
        target_width: u32,
        target_height: u32,
    },
}

impl VideoPlan {
    /// Whether this changes the video stream.
    #[must_use]
    pub fn changes_stream(&self) -> bool {
        matches!(self, Self::Encode { .. })
    }
}

/// Per-track audio decision (index-aligned with the source file's
/// audio tracks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioTrackPlan {
    /// Stream-copy the track.
    Copy,
    /// Drop the track.
    Drop,
    /// Re-encode the track to the target codec.
    Reencode {
        codec: AudioTargetCodec,
        sample_rate: Option<u32>,
        channels: Option<u32>,
        /// Bits/s (from the codec's defaults unless overridden).
        bitrate_bps: Option<u64>,
    },
}

impl AudioTrackPlan {
    /// Whether this changes the track.
    #[must_use]
    pub fn changes_track(&self) -> bool {
        !matches!(self, Self::Copy)
    }
}

/// The audio-domain decision: one decision per source track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioPlan {
    /// Per-track decisions, index-aligned with the source's audio
    /// tracks.
    pub per_track: Vec<AudioTrackPlan>,
}

impl AudioPlan {
    /// Whether any track is re-encoded or dropped.
    #[must_use]
    pub fn changes_anything(&self) -> bool {
        self.per_track.iter().any(|p| p.changes_track())
    }
}

/// The subtitle-domain decision (v1: copies only, §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitlePolicy {
    /// Keep every subtitle track (copy).
    #[default]
    KeepAll,
    /// Keep only forced tracks (drops the rest).
    KeepForced,
    /// Drop every subtitle track.
    Drop,
}

/// The subtitle-domain decision: which source tracks survive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitlePlan {
    /// The policy that produced this decision.
    pub policy: SubtitlePolicy,
    /// The subtitle tracks carried into the output (all copied; in
    /// source order).
    pub tracks: Vec<crate::facts::SubtitleTrack>,
}

impl SubtitlePlan {
    /// Whether any subtitle track is dropped (the only change v1 makes).
    #[must_use]
    pub fn changes_anything(&self) -> bool {
        !matches!(self.policy, SubtitlePolicy::KeepAll)
    }
}

/// A complete, **device-independent** ffmpeg plan for one file
/// (DESIGN §7): what to produce, not who produces it.
///
/// Produced by [`crate::evaluate`] from `(flow, facts)`. The concrete
/// encoder (and its hardware-accel flag) is resolved at dispatch time
/// by [`to_argv`] from the device that will run the job — so the
/// persisted plan is reusable across devices, and CPU is always the
/// working fallback when a GPU is busy or disappears.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FfmpegPlan {
    /// Resolved target container (`"mp4"` | `"mkv"`; also the output
    /// extension, DESIGN §6.3).
    pub container: String,
    /// Video decision (`Copy` = stream-copy).
    pub video: VideoPlan,
    /// Per-track audio decisions (absent = all copied).
    pub audio: Option<AudioPlan>,
    /// Subtitles carried into the output (all copied, v1).
    pub subtitles: Option<SubtitlePlan>,
    /// True when the only change is the container (a remux).
    #[serde(default)]
    pub remux: bool,
}

impl FfmpegPlan {
    /// Whether this plan changes anything at all.
    #[must_use]
    pub fn changes_anything(&self) -> bool {
        self.remux
            || self.video.changes_stream()
            || self.audio.as_ref().is_some_and(|a| a.changes_anything())
            || self.subtitles.as_ref().is_some_and(|s| s.changes_anything())
    }
}

/// The ffmpeg invocation for a plan on a specific device
/// (DESIGN §7, §8).
///
/// `device` selects the video encoder: a device with an encoder for
/// the target codec supplies `<codec>_<accel>` + the `-hwaccel` flag;
/// otherwise the CPU encoder from the plan runs. Audio encoders are
/// always software.
///
/// The returned vector is the ffmpeg argument list (without the
/// binary); the caller prepends the `ffmpeg` path.
///
/// **Note on track indices:** plans use *per-type* stream indices
/// (ffmpeg's `0:<n>` in `-map 0:<type>:<n>`); the server's fact
/// extractor must report per-type indices, not ffprobe's global
/// stream indices.
#[must_use]
pub fn to_argv(plan: &FfmpegPlan, device: &Device, src: &Path, dst: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        src.to_string_lossy().into_owned(),
    ];

    // ── Video ─────────────────────────────────────────────────────
    match &plan.video {
        VideoPlan::Copy => a.push("-c:v".into()),
        VideoPlan::Encode {
            codec,
            encoder,
            profile,
            level,
            bitrate_bps,
            maxrate_bps,
            bufsize_bps,
            crf,
            pix_fmt,
            filters,
            ..
        } => {
            // Device encoder resolution.
            let (encoder, hwaccel) = if device.kind == crate::device::DeviceKind::Gpu {
                device
                    .encoders
                    .iter()
                    .find(|e| e.target == *codec)
                    .map(|e| (e.name.clone(), Some(hwaccel_name(&e.name))))
                    .unwrap_or_else(|| (encoder.clone(), None))
            } else {
                (encoder.clone(), None)
            };
            a.push("-c:v".into());
            a.push(encoder);
            if let Some(h) = hwaccel {
                a.push("-hwaccel".into());
                a.push(h.to_string());
            }
            a.push("-profile:v".into());
            a.push(profile.clone());
            a.push("-level".into());
            a.push(level.clone());
            if let Some(c) = crf {
                a.push("-crf".into());
                a.push(c.to_string());
            } else {
                if let Some(b) = bitrate_bps {
                    a.push("-b:v".into());
                    a.push(b.to_string());
                }
                if let Some(m) = maxrate_bps {
                    a.push("-maxrate".into());
                    a.push(m.to_string());
                }
                if let Some(b) = bufsize_bps {
                    a.push("-bufsize".into());
                    a.push(b.to_string());
                }
            }
            a.push("-pix_fmt".into());
            a.push(pix_fmt.clone());
            if !filters.is_empty() {
                a.push("-vf".into());
                a.push(filters.join(","));
            }
        }
    }

    // ── Audio (per-track) ─────────────────────────────────────────
    if let Some(audio) = &plan.audio {
        for (track, decision) in audio.per_track.iter().enumerate() {
            match decision {
                AudioTrackPlan::Drop => {}
                AudioTrackPlan::Copy => {
                    a.push("-map".into());
                    a.push(format!("0:a:{track}"));
                    a.push("-c:a".into());
                    a.push("copy".into());
                }
                AudioTrackPlan::Reencode {
                    codec,
                    sample_rate,
                    channels,
                    bitrate_bps,
                } => {
                    a.push("-map".into());
                    a.push(format!("0:a:{track}"));
                    a.push("-c:a".into());
                    a.push(codec.encoder().into());
                    if let Some(b) = bitrate_bps {
                        a.push("-b:a".into());
                        a.push(b.to_string());
                    }
                    if let Some(r) = sample_rate {
                        a.push("-ar".into());
                        a.push(r.to_string());
                    }
                    if let Some(c) = channels {
                        a.push("-ac".into());
                        a.push(c.to_string());
                    }
                }
            }
        }
    } else {
        // No audio plan: copy all audio streams.
        a.push("-map".into());
        a.push("0:a".into());
        a.push("-c:a".into());
        a.push("copy".into());
    }

    // ── Subtitles (copy) ──────────────────────────────────────────
    if let Some(subs) = &plan.subtitles {
        if subs.tracks.is_empty() {
            // Drop all subtitles.
            a.push("-sn".into());
        } else {
            for t in &subs.tracks {
                a.push("-map".into());
                a.push(format!("0:s:{}", t.index));
                a.push("-c:s".into());
                a.push("copy".into());
            }
        }
    } else {
        a.push("-map".into());
        a.push("0:s".into());
        a.push("-c:s".into());
        a.push("copy".into());
    }

    // ── Output ────────────────────────────────────────────────────
    if plan.container.eq_ignore_ascii_case("mp4") {
        a.push("-movflags".into());
        a.push("+faststart".into());
    }
    a.push(dst.to_string_lossy().into_owned());
    a
}

/// Hardware-accel flag for an encoder name (`h264_nvenc` → `cuda`).
fn hwaccel_name(encoder: &str) -> &'static str {
    let (_, accel) = hwaccel_for(encoder);
    accel.unwrap_or("cuda")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu() -> Device {
        Device::cpu(2)
    }

    fn plan() -> FfmpegPlan {
        FfmpegPlan {
            container: "mp4".into(),
            video: VideoPlan::Encode {
                codec: VideoTargetCodec::H264,
                encoder: "libx264".into(),
                profile: "high".into(),
                level: "4.2".into(),
                bitrate_bps: Some(12_000_000),
                maxrate_bps: Some(13_800_000),
                bufsize_bps: Some(24_000_000),
                crf: None,
                pix_fmt: "yuv420p".into(),
                filters: vec!["scale=1920:1080:flags=lanczos".into()],
                target_width: 1920,
                target_height: 1080,
            },
            audio: Some(AudioPlan {
                per_track: vec![
                    AudioTrackPlan::Copy,
                    AudioTrackPlan::Reencode {
                        codec: AudioTargetCodec::Eac3,
                        sample_rate: Some(48_000),
                        channels: Some(6),
                        bitrate_bps: Some(640_000),
                    },
                    AudioTrackPlan::Drop,
                ],
            }),
            subtitles: Some(SubtitlePlan {
                policy: SubtitlePolicy::KeepAll,
                tracks: vec![crate::facts::SubtitleTrack {
                    index: 0,
                    codec: "mov_text".into(),
                    ..Default::default()
                }],
            }),
            remux: true,
        }
    }

    #[test]
    fn cpu_argv_is_device_independent() {
        let argv = to_argv(&plan(), &cpu(), Path::new("/in.mkv"), Path::new("/out.mp4"));
        let s = argv.join(" ");
        assert!(s.contains("-c:v libx264"), "{s}");
        assert!(s.contains("-profile:v high"), "{s}");
        assert!(s.contains("-level 4.2"), "{s}");
        assert!(s.contains("-b:v 12000000"), "{s}");
        assert!(s.contains("-vf scale=1920:1080:flags=lanczos"), "{s}");
        // Track 0 copied, track 1 re-encoded, track 2 dropped.
        assert!(s.contains("0:a:0"), "{s}");
        assert!(s.contains("-c:a libeac3"), "{s}");
        assert!(!s.contains("0:a:2"), "dropped track must not be mapped: {s}");
        assert!(s.contains("0:s:0"), "{s}");
        assert!(s.contains("-c:s copy"), "{s}");
        assert!(s.contains("+faststart"), "{s}");
        assert!(s.ends_with("/out.mp4"), "{s}");
    }

    #[test]
    fn gpu_device_swaps_encoder_and_hwaccel() {
        let mut gpu = Device {
            id: "gpu-nvidia-0".into(),
            kind: crate::device::DeviceKind::Gpu,
            name: "NVIDIA".into(),
            encoders: vec![
                crate::device::Encoder {
                    name: "h264_nvenc".into(),
                    target: VideoTargetCodec::H264,
                },
                crate::device::Encoder {
                    name: "hevc_nvenc".into(),
                    target: VideoTargetCodec::Hevc,
                },
            ],
            max_concurrent: 2,
        };
        let argv = to_argv(&plan(), &gpu, Path::new("/in.mkv"), Path::new("/out.mp4"));
        let s = argv.join(" ");
        assert!(s.contains("-c:v h264_nvenc"), "{s}");
        assert!(s.contains("-hwaccel cuda"), "{s}");
        let _ = &mut gpu;
    }
}
