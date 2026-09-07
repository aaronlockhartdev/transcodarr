use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::plan::{AudioPlan, AudioTrackPlan, VideoPlan};
use crate::registry::OperationSection;
use crate::registry::SectionPlan;
use crate::registry::operation::parse;
use crate::registry::operation::video::{ContainerChoice, ContainerSpec};

/// MP4-safe subtitle codecs (text-based). Bitmap subs (e.g. `hdmv_pgs_subtitle`)
/// force MKV.
fn mp4_safe_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "mov_text" | "srt" | "webvtt" | "subrip" | "ass" | "ssa"
    )
}

/// True when every **planned surviving** stream fits an MP4-family
/// container (MP4 or MOV — same muxer family, same stream rules).
///
/// Judged by the *planned* codec: an encoded video counts by its
/// **target** codec; a re-encoded audio track counts by its target
/// codec; dropped tracks are skipped.
fn mp4_safe(facts: &FileFacts, video: &Option<VideoPlan>, audio: &Option<AudioPlan>) -> bool {
    // Video.
    let video_codec = match video.as_ref() {
        Some(VideoPlan::Encode(e)) => Some(e.codec.name().to_ascii_lowercase()),
        _ => facts.video.as_ref().map(|v| v.codec.to_ascii_lowercase()),
    };
    if let Some(vc) = video_codec
        && !matches!(vc.as_str(), "h264" | "hevc" | "avc" | "h265")
    {
        return false;
    }
    // Audio.
    for (i, track) in facts.audio.iter().enumerate() {
        let plan = audio
            .as_ref()
            .and_then(|a| a.per_track.get(i))
            .unwrap_or(&AudioTrackPlan::Copy);
        let codec = match plan {
            AudioTrackPlan::Reencode { codec, .. } => codec.name().to_ascii_lowercase(),
            AudioTrackPlan::Drop => continue,
            _ => track.codec.to_ascii_lowercase(),
        };
        if !matches!(codec.as_str(), "eac3" | "ac3" | "aac") {
            return false;
        }
    }
    // Subtitles — bitmap codecs force MKV.
    facts.subtitles.iter().all(|s| mp4_safe_subtitle(&s.codec))
}

/// Resolve a container spec to the target container (DESIGN §6.4):
///
/// - `mp4` / `mov`: MP4 family. If every planned stream is MP4-safe the
///   chosen container is used; otherwise the **fallback** decides — on,
///   MKV; off, an error (the file fails at plan time with a clear
///   reason; nothing is encoded).
/// - `mkv`: the superset container, verbatim (everything fits).
/// - `webm`: verbatim — there is no plan-time stream matrix for it, so
///   an incompatible stream fails at encode time with ffmpeg's own
///   error (the job log shows it).
pub fn resolve(
    spec: &ContainerSpec,
    facts: &FileFacts,
    video: &Option<VideoPlan>,
    audio: &Option<AudioPlan>,
) -> crate::error::Result<&'static str> {
    match spec.choice {
        ContainerChoice::Mp4 | ContainerChoice::Mov => {
            let target = match spec.choice {
                ContainerChoice::Mp4 => "mp4",
                ContainerChoice::Mov => "mov",
                _ => unreachable!("mp4 family only"),
            };
            if mp4_safe(facts, video, audio) {
                Ok(target)
            } else if spec.fallback {
                Ok("mkv")
            } else {
                Err(crate::error::CoreError::ContainerIncompatible { target })
            }
        }
        ContainerChoice::Mkv => Ok("mkv"),
        ContainerChoice::Webm => Ok("webm"),
    }
}

/// The `container` section.
///
/// A container choice changes no stream — only the wrapper, so this
/// section always plans [`SectionPlan::Identity`]. The effective
/// container is resolved at the
/// [`Operation`](crate::flow::Operation) level: the top-level `container`
/// field (where this section's UI control writes its value — a top-level
/// `"container"` key is consumed by that named field and never reaches
/// the sections map) or the `container` field nested in the `video`
/// section (top-level wins). An explicit choice that resolves to a
/// container different from the source triggers a pure remux even on a
/// stream-identical step; the default spec (MP4 + fallback — the old
/// `smart`) is only a resolution input and never forces a remux by
/// itself.
///
/// Flow JSON: `{ "container": { "choice": "mp4"|"mkv"|"webm"|"mov", "fallback": true|false } }`
/// (legacy string forms upgrade in place — DESIGN §13.11).
pub struct Container;

/// The `container` section parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerOp {
    #[serde(default)]
    pub choice: ContainerChoice,
    #[serde(default = "default_true")]
    pub fallback: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ContainerOp {
    fn default() -> Self {
        Self {
            choice: ContainerChoice::default(),
            fallback: true,
        }
    }
}

impl OperationSection for Container {
    fn key(&self) -> &'static str {
        "container"
    }

    fn description(&self) -> &'static str {
        "Target container: an explicit choice (mp4, mkv, webm, mov) with an optional fallback to MKV when a stream cannot fit"
    }

    fn plan(&self, params: &Value, _facts: &FileFacts) -> crate::error::Result<SectionPlan> {
        // Container changes no stream: identity. (The remux effect is
        // resolved at the Operation level — see `resolve`.)
        let _op: ContainerOp = parse(self.key(), params)?;
        Ok(SectionPlan::Identity)
    }

    fn validate(&self, params: &Value) -> crate::error::Result<()> {
        parse::<ContainerOp>(self.key(), params).map(|_| ())
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "object",
            "label": "Container",
            "fields": {
                "choice": {
                    "kind": "single_select",
                    "label": "Container",
                    "values": [
                        { "value": "mp4", "label": "MP4" },
                        { "value": "mkv", "label": "MKV" },
                        { "value": "webm", "label": "WebM" },
                        { "value": "mov", "label": "MOV" }
                    ],
                    "default": "mp4",
                    "hint": "The wrapper every output file gets."
                },
                "fallback": {
                    "kind": "boolean",
                    "label": "Fall back to MKV",
                    "default": true,
                    "hint": "If a stream cannot go into the chosen container, use MKV instead. Off: the file fails."
                }
            },
            "hint": "The default (MP4 with the fallback on) is the old Auto: MP4 when safe, else MKV."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{AudioTrack, SubtitleTrack};
    use crate::plan::{AudioTargetCodec, VideoEncode, VideoTargetCodec};

    fn facts(container: &str, video: &str, audio: &[&str], subs: &[&str]) -> FileFacts {
        FileFacts {
            container: container.into(),
            video: Some(crate::facts::VideoFacts {
                codec: video.into(),
                ..Default::default()
            }),
            audio: audio
                .iter()
                .map(|c| AudioTrack {
                    codec: (*c).to_string(),
                    ..Default::default()
                })
                .collect(),
            subtitles: subs
                .iter()
                .map(|c| SubtitleTrack {
                    codec: (*c).to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    /// The old `smart` behaviour, as the default spec.
    fn smart() -> ContainerSpec {
        ContainerSpec::default()
    }

    #[test]
    fn smart_is_mp4_for_safe_codecs() {
        let f = facts("mkv", "h264", &["eac3"], &["mov_text"]);
        assert_eq!(resolve(&smart(), &f, &None, &None).ok(), Some("mp4"));
    }

    #[test]
    fn smart_is_mkv_for_unsafe_video() {
        let f = facts("mkv", "av1", &["aac"], &[]);
        assert_eq!(resolve(&smart(), &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn smart_is_mkv_for_unsafe_audio() {
        let f = facts("mkv", "h264", &["dts"], &[]);
        assert_eq!(resolve(&smart(), &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn smart_is_mkv_for_unsafe_subtitles() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        assert_eq!(resolve(&smart(), &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn reencoded_audio_counts_as_target_codec() {
        // dts source → eac3 target: smart is MP4 (eac3 is MP4-safe),
        // even though the source codec isn't.
        let f = facts("mkv", "h264", &["dts"], &[]);
        let audio = Some(AudioPlan {
            per_track: vec![AudioTrackPlan::Reencode {
                codec: AudioTargetCodec::Eac3,
                sample_rate: None,
                channels: None,
                bitrate_bps: None,
            }],
            filter: None,
        });
        assert_eq!(resolve(&smart(), &f, &None, &audio).ok(), Some("mp4"));
    }

    #[test]
    fn dropped_tracks_dont_affect_safety() {
        let f = facts("mkv", "h264", &["dts", "truehd"], &[]);
        let audio = Some(AudioPlan {
            per_track: vec![
                AudioTrackPlan::Reencode {
                    codec: AudioTargetCodec::Aac,
                    sample_rate: None,
                    channels: None,
                    bitrate_bps: None,
                },
                AudioTrackPlan::Drop,
            ],
            filter: None,
        });
        assert_eq!(resolve(&smart(), &f, &None, &audio).ok(), Some("mp4"));
    }

    #[test]
    fn reencoded_video_counts_as_target_codec() {
        // vp9 source → h264 target: smart is MP4 even though the
        // source codec isn't MP4-safe.
        let f = facts("mkv", "vp9", &["aac"], &[]);
        let video = Some(VideoPlan::Encode(Box::new(VideoEncode {
            codec: VideoTargetCodec::H264,
            encoder: "libx264".into(),
            profile: "high".into(),
            level: "4.2".into(),
            bitrate_bps: Some(8_000_000),
            maxrate_bps: Some(9_200_000),
            bufsize_bps: Some(16_000_000),
            crf: None,
            pix_fmt: "yuv420p".into(),
            filters: vec![],
            filter: None,
            target_width: 3840,
            target_height: 2160,
        })));
        assert_eq!(resolve(&smart(), &f, &video, &None).ok(), Some("mp4"));
    }

    #[test]
    fn copied_unsafe_video_still_forces_mkv() {
        // vp9 with no video plan (Copy) stays MKV.
        let f = facts("mkv", "vp9", &["aac"], &[]);
        assert_eq!(resolve(&smart(), &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn explicit_choice_passes_through() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        let mkv = ContainerSpec {
            choice: ContainerChoice::Mkv,
            fallback: false,
        };
        let mp4 = ContainerSpec {
            choice: ContainerChoice::Mp4,
            fallback: true,
        };
        assert_eq!(resolve(&mkv, &f, &None, &None).ok(), Some("mkv"));
        // mp4 with fallback on and unsafe subs → mkv.
        assert_eq!(resolve(&mp4, &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn explicit_mp4_without_fallback_errors_on_unsafe_streams() {
        let f = facts("mkv", "h264", &["dts"], &[]);
        let spec = ContainerSpec {
            choice: ContainerChoice::Mp4,
            fallback: false,
        };
        let err = resolve(&spec, &f, &None, &None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::CoreError::ContainerIncompatible { target: "mp4" }
        ));
    }

    #[test]
    fn explicit_mov_without_fallback_errors_on_unsafe_streams() {
        let f = facts("mkv", "av1", &["aac"], &[]);
        let spec = ContainerSpec {
            choice: ContainerChoice::Mov,
            fallback: false,
        };
        assert!(matches!(
            resolve(&spec, &f, &None, &None).unwrap_err(),
            crate::error::CoreError::ContainerIncompatible { target: "mov" }
        ));
    }

    #[test]
    fn mov_with_fallback_falls_back_to_mkv() {
        let f = facts("mp4", "av1", &["dts"], &[]);
        let spec = ContainerSpec {
            choice: ContainerChoice::Mov,
            fallback: true,
        };
        assert_eq!(resolve(&spec, &f, &None, &None).ok(), Some("mkv"));
    }

    #[test]
    fn webm_resolves_verbatim_no_plan_time_check() {
        let f = facts("mkv", "h264", &["dts"], &[]);
        let spec = ContainerSpec {
            choice: ContainerChoice::Webm,
            fallback: true,
        };
        // No webm stream matrix: the choice is trusted (an
        // incompatible stream fails at ffmpeg encode time instead).
        assert_eq!(resolve(&spec, &f, &None, &None).ok(), Some("webm"));
    }

    #[test]
    fn legacy_strings_upgrade() {
        assert_eq!(
            serde_json::from_value::<ContainerSpec>(json!("smart")).unwrap(),
            smart()
        );
        assert_eq!(
            serde_json::from_value::<ContainerSpec>(json!("mp4")).unwrap(),
            ContainerSpec {
                choice: ContainerChoice::Mp4,
                fallback: false
            }
        );
        assert_eq!(
            serde_json::from_value::<ContainerSpec>(json!("mkv")).unwrap(),
            ContainerSpec {
                choice: ContainerChoice::Mkv,
                fallback: false
            }
        );
        // Unknown value: lenient default (matches the old handling).
        assert_eq!(
            serde_json::from_value::<ContainerSpec>(json!("bogus")).unwrap(),
            smart()
        );
        // Object form round-trips.
        let spec = ContainerSpec {
            choice: ContainerChoice::Webm,
            fallback: false,
        };
        let v = serde_json::to_value(spec).unwrap();
        assert_eq!(v, json!({ "choice": "webm", "fallback": false }));
        assert_eq!(serde_json::from_value::<ContainerSpec>(v).unwrap(), spec);
        // Unknown fields are rejected at validation.
        assert!(
            serde_json::from_value::<ContainerSpec>(json!({"choice":"mp4","bogus":1})).is_err()
        );
    }
}
