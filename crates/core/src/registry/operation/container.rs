use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::plan::{AudioPlan, AudioTrackPlan, VideoPlan};
use crate::registry::OperationSection;
use crate::registry::SectionPlan;
use crate::registry::operation::parse;
use crate::registry::operation::video::ContainerChoice;

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
/// - `smart`: MP4 if every *surviving* stream is MP4-safe (video h264/hevc
///   — an encoded video counts by its **target** codec; audio eac3/ac3/aac
///   — re-encoded tracks count by their **target** codec, dropped tracks
///   are skipped; subtitles text-based), else MKV.
/// - `mp4` / `mkv`: the named container, verbatim.
#[must_use]
pub fn resolve(
    choice: ContainerChoice,
    facts: &FileFacts,
    video: &Option<VideoPlan>,
    audio: &Option<AudioPlan>,
) -> &'static str {
    match choice {
        ContainerChoice::Mp4 => "mp4",
        ContainerChoice::Mkv => "mkv",
        ContainerChoice::Smart => {
            // Video — judged by the planned (target) codec: an encode
            // to h264/hevc is MP4-safe even when the source codec isn't.
            let video_codec = match video.as_ref() {
                Some(VideoPlan::Encode(e)) => Some(e.codec.name().to_ascii_lowercase()),
                _ => facts.video.as_ref().map(|v| v.codec.to_ascii_lowercase()),
            };
            if let Some(vc) = video_codec {
                if !matches!(vc.as_str(), "h264" | "hevc" | "avc" | "h265") {
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
            if !facts.subtitles.iter().all(|s| mp4_safe_subtitle(&s.codec)) {
                return "mkv";
            }
            "mp4"
        }
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
/// section (top-level wins). An **explicit** choice (`mp4`/`mkv`) that
/// resolves to a container different from the source's triggers a pure
/// remux even on a stream-identical step; **smart** is only a
/// resolution input and never forces a remux by itself (DESIGN §13.7).
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
            "label": "Container",
            "values": [
                { "value": "smart", "label": "Auto (MP4 when safe, else MKV)" },
                { "value": "mp4", "label": "MP4" },
                { "value": "mkv", "label": "MKV" }
            ],
            "default": "smart",
            "hint": "Forces this container. Auto only picks between MP4 and MKV."
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

    #[test]
    fn smart_is_mp4_for_safe_codecs() {
        let f = facts("mkv", "h264", &["eac3"], &["mov_text"]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &None), "mp4");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_video() {
        let f = facts("mkv", "av1", &["aac"], &[]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &None), "mkv");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_audio() {
        let f = facts("mkv", "h264", &["dts"], &[]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &None), "mkv");
    }

    #[test]
    fn smart_is_mkv_for_unsafe_subtitles() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &None), "mkv");
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
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &audio), "mp4");
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
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &audio), "mp4");
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
        assert_eq!(resolve(ContainerChoice::Smart, &f, &video, &None), "mp4");
    }

    #[test]
    fn copied_unsafe_video_still_forces_mkv() {
        // vp9 with no video plan (Copy) stays MKV.
        let f = facts("mkv", "vp9", &["aac"], &[]);
        assert_eq!(resolve(ContainerChoice::Smart, &f, &None, &None), "mkv");
    }

    #[test]
    fn explicit_choice_passes_through() {
        let f = facts("mp4", "h264", &["aac"], &["hdmv_pgs"]);
        assert_eq!(resolve(ContainerChoice::Mkv, &f, &None, &None), "mkv");
        assert_eq!(resolve(ContainerChoice::Mp4, &f, &None, &None), "mp4");
    }
}
