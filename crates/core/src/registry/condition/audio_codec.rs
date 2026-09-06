use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::registry::ConditionField;
use crate::registry::condition::parse;

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
        Ok(facts
            .audio
            .iter()
            .any(|t| c.any_of.iter().any(|w| w.eq_ignore_ascii_case(&t.codec))))
    }

    fn ui_schema(&self) -> Value {
        let values = codec_value_pairs();
        json!({
            "kind": "multi_select",
            "label": "Audio codec",
            "values": values,
            "hint": "The file matches when any audio track uses one of these codecs. Leave empty for any."
        })
    }
}

/// The source audio codec vocabulary shared by the `audio_codec` condition
/// and the audio operation's rule matching (one place, both consumers).
pub const AUDIO_CODECS: [&str; 10] = [
    "eac3", "eac3_joc", "ac3", "dts", "dts_ma", "truehd", "aac", "flac", "mp3", "opus",
];

/// Display names for [`AUDIO_CODECS`] (same order; the schema renders
/// these, the wire format keeps the raw codec names).
pub const AUDIO_CODEC_LABELS: [&str; 10] = [
    "E-AC-3",
    "E-AC-3 (Atmos)",
    "AC-3",
    "DTS",
    "DTS-HD MA",
    "TrueHD",
    "AAC",
    "FLAC",
    "MP3",
    "Opus",
];

/// Zip the codec vocabulary with its display names into schema pairs.
#[must_use]
pub fn codec_value_pairs() -> Vec<Value> {
    AUDIO_CODECS
        .iter()
        .zip(AUDIO_CODEC_LABELS.iter())
        .map(|(value, label)| json!({ "value": value, "label": label }))
        .collect()
}

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
        assert!(
            c.match_facts(&json!({ "any_of": [] }), &facts(&[]))
                .unwrap()
        );
    }
}
