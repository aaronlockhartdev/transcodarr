use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::facts::FileFacts;
use crate::plan::{AudioPlan, AudioTrackPlan};
use crate::registry::operation::parse;
use crate::registry::operation::video::ContainerChoice;
use crate::registry::OperationSection;
use crate::registry::SectionPlan;

/// MP4-safe subtitle codecs (text-based). Bitmap subs (e.g. `hdmv_pgs_subtitle`)
/// force MKV.
fn mp4_safe_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "mov_text" | "srt" | "webvtt" | "subrip" | "ass" | "ssa"
    )
}

/// The target container implied by a container choice (DESIGN §13.7):
///
/// - `smart`: MP4 if every *surviving* stream is MP4-safe (video codec
///   h264/hevc; audio eac3/ac3/aac — re-encoded tracks count by their
///   **target** codec, dropped tracks are skipped; subtitles text-based),
///   else MKV.
/// - `mp4` / `mkv`: the named container, verbatim.
#[must_use]
pub fn resolve(choice: ContainerChoice, facts: &FileFacts, audio: &Option<AudioPlan>) -> &'static str {
    match choice {
        ContainerChoice::Mp4 => "mp4",
        ContainerChoice::Mkv => "mkv",
        ContainerChoice::Smart => {
            // Video.
            if let Some(v) = facts.video() {
                if !matches!(v.codec.to_ascii_lowercase().as_str(), "h264" | "hevc" | "avc" | "h265") {
                    return "mkv";
                }
            }
            // Audio — judged by the planned (target) codec.
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
                    return "mkv";
                }
            }
            // Subtitles — bitmap codecs force MKV.
            if !facts
                .subtitles
                .iter()
                .all(|s| mp4_safe_subtitle(&s.codec))
            {
                return "mkv";
            }
            "mp4"
        }
    }
}

/// The `container` section.
///
/// A container choice changes no stream — only the wrapper. For that
/// reason this section always plans [`SectionPlan::Identity`]; the
/// effective container is resolved by the evaluator from the
/// [`Operation`](crate::flow::Operation) level (the `container` field,
/// top-level or nested in `video`). The section exists so the flow
/// schema/UI can advertise the choice and so an explicit
/// `{"container": {"choice": …}}` form is accepted (and validated).
///
/// Flow JSON: `{ "container": { "choice": "smart" | "mp4" | "mkv" } }`
pub struct Container;

/// The `container` section parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerOp {
    #[serde(default)]
    pub choice: ContainerChoice,
}

impl Default for ContainerOp {
    fn default() -> Self {
        Self {
            choice: ContainerChoice::Smart,
        }
    }
}

impl OperationSection for Container {
    fn key(&self) -> &'static str {
        "container"
    }

    fn description(&self) -> &'static str {
        "Target container: smart (MP4 if safe, else MKV), mp4, or mkv"
    }

    fn plan(&self, params: &Value, _facts: &FileFacts) -> crate::error::Result<SectionPlan> {
        // Container changes no stream: identity. (The remux effect is
        // resolved at the Operation level — see `resolve`.)
        let _op: ContainerOp = parse(self.key(), params)?;
        Ok(SectionPlan::Identity)
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "single_select",
            "values": [
                { "value": "smart", "label": "Smart — MP4 if MP4-safe, else MKV" },
                { "value": "mp4", "label": "MP4" },
                { "value": "mkv", "label": "MKV" }
            ],
            "default": "smart"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{AudioTrack, SubtitleTrack};
    use crate::plan::AudioTargetCodec;

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

    #[test]
    fn smart_is_mp4_for_safe_codecs() {
        let f = facts("mkv", "h264", &["eac3"], &["mov_text"]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None), "mp4");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_video() {
        let f = facts("mkv", "av1", &["aac"], &[]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None), "mkv");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_audio() {
        let f = facts("mkv", "h264", &["dts"], &[]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None), "mkv");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_subtitles() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None), "mkv");
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
        });
        assert_eq!(resolve(ContainerChoice::Smart, &f, &audio), "mp4");
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
        });
        assert_eq!(resolve(ContainerChoice::Smart, &f, &audio), "mp4");
    }

    #[test]
    fn explicit_choice_passes_through() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        assert_eq!(resolve(ContainerChoice::Mkv, &f, &None), "mkv");
        assert_eq!(resolve(ContainerChoice::Mp4, &f, &None), "mp4");
    }
}
