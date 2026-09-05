use serde::Deserialize;
use serde_json::{json, Value};

use crate::facts::FileFacts;
use crate::registry::condition::parse;
use crate::registry::ConditionField;

/// `audio_codec` — matches when **any** audio track has one of the
/// listed codecs (DESIGN §5: "audio codec set").
///
/// Flow JSON: `{ "audio_codec": { "any_of": ["dts", "truehd"] } }`.
/// This is the classic "which files carry DTS/TrueHD" selector for
/// audio-conversion steps.
pub struct AudioCodec;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    #[serde(default)]
    any_of: Vec<String>,
}

impl ConditionField for AudioCodec {
    fn key(&self) -> &'static str {
        "audio_codec"
    }

    fn description(&self) -> &'static str {
        "True when any audio track uses one of these codecs"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.any_of.is_empty() {
            return Ok(true);
        }
        Ok(facts.audio.iter().any(|t| {
            c.any_of
                .iter()
                .any(|w| w.eq_ignore_ascii_case(&t.codec))
        }))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "multi_select",
            "values": AUDIO_CODECS,
            "hint": "Matches if ANY track has a listed codec. Leave empty for any.",
        })
    }
}

/// The source audio codec vocabulary shared by the `audio_codec` condition
/// and the audio operation's rule matching (one place, both consumers).
pub const AUDIO_CODECS: [&str; 10] =
    ["eac3", "eac3_joc", "ac3", "dts", "dts_ma", "truehd", "aac", "flac", "mp3", "opus"];
#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::AudioTrack;

    fn facts(codecs: &[&str]) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            audio: codecs
                .iter()
                .enumerate()
                .map(|(i, c)| AudioTrack {
                    index: i as u32,
                    codec: (*c).into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn any_track_matches() {
        let c = AudioCodec;
        let v = json!({ "any_of": ["dts", "truehd"] });
        assert!(c.match_facts(&v, &facts(&["aac", "dts"])).unwrap());
        assert!(!c.match_facts(&v, &facts(&["eac3", "aac"])).unwrap());
    }

    #[test]
    fn empty_is_any() {
        let c = AudioCodec;
        assert!(c.match_facts(&json!({ "any_of": [] }), &facts(&[])).unwrap());
    }
}
