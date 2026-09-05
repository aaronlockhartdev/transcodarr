//! Output verification (DESIGN §3.3): the pure comparison that decides
//! whether a finished output file matches the plan **before** the
//! original is touched.
//!
//! The I/O-bound checks (probing the output) live in
//! `transcodarr-server` behind the [`crate::registry::VerificationCheck`]
//! trait; this module holds the decision logic, so it is unit-testable
//! with facts alone.

use crate::error::CoreError;
use crate::facts::FileFacts;
use crate::plan::{FfmpegPlan, VideoPlan};

/// Default duration tolerance window: ±5% of the source duration,
/// floored at 1 s. Short files (clips) can legitimately drift a bit
/// more in absolute seconds than long ones.
fn duration_tolerance_s(source: f64) -> f64 {
    (source * 0.05).max(1.0)
}

/// Verify `output` against the plan and the source `input` (DESIGN §3.3).
///
/// Checks, in order:
/// 1. **Container** matches the plan's resolved target.
/// 2. **Duration** is within the tolerance window of the source
///    (truncation is fine; a big delta means a broken encode).
/// 3. **Stream inventory**: the audio and subtitle track counts match
///    the plan (dropped tracks absent; re-encoded ones present).
/// 4. **Video** matches the plan: copied video must be present and
///    keep its codec (a missing stream is a data-loss failure, never
///    a pass), re-encoded video must be present in the target codec,
///    and a file with no input video must not gain one.
///
/// Returns `Ok(true)` when every check passes; `Ok(false)` (with the
/// first failure named) otherwise.
#[must_use]
pub fn verify_output(
    plan: &FfmpegPlan,
    input: &FileFacts,
    output: &FileFacts,
) -> Result<bool, CoreError> {
    // 1. Container.
    if !output.container.eq_ignore_ascii_case(&plan.container) {
        return Ok(false);
    }
    // 2. Duration.
    if input.duration_s > 0.0 && output.duration_s > 0.0 {
        if (output.duration_s - input.duration_s).abs() > duration_tolerance_s(input.duration_s) {
            return Ok(false);
        }
    }
    // 3. Stream inventory.
    let expected_audio = match &plan.audio {
        Some(a) => a
            .per_track
            .iter()
            .filter(|p| !matches!(p, crate::plan::AudioTrackPlan::Drop))
            .count(),
        None => input.audio.len(),
    };
    if output.audio.len() != expected_audio {
        return Ok(false);
    }
    let expected_subs = match &plan.subtitles {
        Some(s) => s.tracks.len(),
        None => input.subtitles.len(),
    };
    if output.subtitles.len() != expected_subs {
        return Ok(false);
    }
    // 4. Video.
    match &plan.video {
        None => {
            // No video in the input: the output must not gain any.
            if output.video.is_some() {
                return Ok(false);
            }
        }
        Some(VideoPlan::Copy) => {
            // Video was copied: it must still be present, with the
            // same codec. A missing stream here is the classic
            // silent video-loss failure — it must fail, never pass.
            let (Some(in_v), Some(out_v)) = (&input.video, &output.video) else {
                return Ok(false);
            };
            if !in_v.codec.eq_ignore_ascii_case(&out_v.codec) {
                return Ok(false);
            }
        }
        Some(VideoPlan::Encode { codec, .. }) => {
            let Some(out_v) = &output.video else {
                return Ok(false);
            };
            if !out_v.codec.eq_ignore_ascii_case(codec.name()) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{AudioTrack, VideoFacts};
    use crate::plan::{AudioPlan, AudioTrackPlan, VideoTargetCodec};

    fn input_facts() -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            video: Some(VideoFacts {
                codec: "hevc".into(),
                width: 3840,
                height: 2160,
                ..Default::default()
            }),
            audio: vec![
                AudioTrack {
                    codec: "eac3_joc".into(),
                    ..Default::default()
                },
                AudioTrack {
                    codec: "dts".into(),
                    ..Default::default()
                },
            ],
            subtitles: vec![crate::facts::SubtitleTrack {
                codec: "mov_text".into(),
                ..Default::default()
            }],
            duration_s: 7200.0,
            ..Default::default()
        }
    }

    fn plan() -> FfmpegPlan {
        FfmpegPlan {
            container: "mp4".into(),
            video: Some(VideoPlan::Encode {
                codec: VideoTargetCodec::H264,
                encoder: "libx264".into(),
                profile: "high".into(),
                level: "4.2".into(),
                bitrate_bps: Some(12_000_000),
                maxrate_bps: None,
                bufsize_bps: None,
                crf: None,
                pix_fmt: "yuv420p".into(),
                filters: vec![],
                target_width: 1920,
                target_height: 1080,
            }),
            audio: Some(AudioPlan {
                per_track: vec![AudioTrackPlan::Copy, AudioTrackPlan::Drop],
            }),
            subtitles: None,
            remux: true,
        }
    }

    fn output_facts() -> FileFacts {
        FileFacts {
            container: "mp4".into(),
            video: Some(VideoFacts {
                codec: "h264".into(),
                width: 1920,
                height: 1080,
                ..Default::default()
            }),
            audio: vec![AudioTrack {
                codec: "eac3_joc".into(),
                ..Default::default()
            }],
            subtitles: vec![crate::facts::SubtitleTrack {
                codec: "mov_text".into(),
                ..Default::default()
            }],
            duration_s: 7198.0,
            ..Default::default()
        }
    }

    #[test]
    fn good_output_passes() {
        assert!(verify_output(&plan(), &input_facts(), &output_facts()).unwrap());
    }

    #[test]
    fn wrong_codec_fails() {
        let mut out = output_facts();
        out.video.as_mut().unwrap().codec = "hevc".into();
        assert!(!verify_output(&plan(), &input_facts(), &out).unwrap());
    }

    #[test]
    fn missing_dropped_track_passes_and_extra_fails() {
        let mut out = output_facts();
        // A second audio track reappears → inventory mismatch.
        out.audio.push(AudioTrack {
            codec: "dts".into(),
            ..Default::default()
        });
        assert!(!verify_output(&plan(), &input_facts(), &out).unwrap());
    }

    #[test]
    fn duration_drift_fails() {
        let mut out = output_facts();
        out.duration_s = 6000.0; // 20% short → well past the 5% window
        assert!(!verify_output(&plan(), &input_facts(), &out).unwrap());
    }

    #[test]
    fn small_drift_passes() {
        let mut out = output_facts();
        out.duration_s = 7200.0 + 3.0; // +0.04% → fine
        assert!(verify_output(&plan(), &input_facts(), &out).unwrap());
    }

    #[test]
    fn container_mismatch_fails() {
        let mut out = output_facts();
        out.container = "mkv".into();
        assert!(!verify_output(&plan(), &input_facts(), &out).unwrap());
    }

    #[test]
    fn copied_video_missing_from_output_fails() {
        // The P0: a copy plan whose output lost the video stream is
        // silent data loss — verification must fail, so the original
        // is never swapped out.
        let mut p = plan();
        p.video = Some(VideoPlan::Copy);
        let mut out = output_facts();
        out.video = None;
        assert!(!verify_output(&p, &input_facts(), &out).unwrap());
    }

    #[test]
    fn copied_video_keeps_source_codec_passes() {
        let mut p = plan();
        p.video = Some(VideoPlan::Copy);
        let mut out = output_facts();
        out.video = Some(VideoFacts {
            codec: "hevc".into(),
            ..Default::default()
        });
        assert!(verify_output(&p, &input_facts(), &out).unwrap());
    }
}
