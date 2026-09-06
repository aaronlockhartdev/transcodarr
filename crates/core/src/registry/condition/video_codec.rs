use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::registry::ConditionField;
use crate::registry::condition::parse;

/// `video_codec` — matches the file's video codec.
///
/// Flow JSON: `{ "video_codec": { "in": ["hevc"] } }`. A file without a
/// video stream never matches a non-"any" constraint.
pub struct VideoCodec;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    #[serde(default)]
    r#in: Vec<String>,
}

impl ConditionField for VideoCodec {
    fn key(&self) -> &'static str {
        "video_codec"
    }

    fn description(&self) -> &'static str {
        "Video codec (hevc, h264, …)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.r#in.is_empty() {
            return Ok(true);
        }
        let Some(v) = facts.video() else {
            return Ok(false);
        };
        Ok(c.r#in.iter().any(|w| w.eq_ignore_ascii_case(&v.codec)))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "multi_select",
            "label": "Video codec",
            "values": [
                { "value": "h264", "label": "H.264" },
                { "value": "hevc", "label": "HEVC" },
                { "value": "av1", "label": "AV1" },
                { "value": "vp9", "label": "VP9" },
                { "value": "mpeg2video", "label": "MPEG-2" },
                { "value": "mpeg4", "label": "MPEG-4" }
            ],
            "hint": "The file must use one of these video codecs. Leave empty for any."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::VideoFacts;

    fn facts(codec: &str) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            video: Some(VideoFacts {
                codec: codec.into(),
                width: 1920,
                height: 1080,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn matches_codec() {
        let c = VideoCodec;
        let v = json!({ "in": ["hevc"] });
        assert!(c.match_facts(&v, &facts("hevc")).unwrap());
        assert!(!c.match_facts(&v, &facts("h264")).unwrap());
    }

    #[test]
    fn no_video_never_matches() {
        let c = VideoCodec;
        let v = json!({ "in": ["hevc"] });
        assert!(
            !c.match_facts(
                &v,
                &FileFacts {
                    container: "mp3".into(),
                    ..Default::default()
                }
            )
            .unwrap()
        );
        // but "any" still matches a video-less file
        assert!(
            c.match_facts(
                &json!({}),
                &FileFacts {
                    container: "mp3".into(),
                    ..Default::default()
                }
            )
            .unwrap()
        );
    }
}
