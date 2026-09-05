use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::plan::AudioPlan;
use crate::registry::operation::parse;
use crate::registry::{OperationSection, SectionPlan};

/// The v1 audio codec target, shared with the plan model.
pub use crate::plan::{AudioTargetCodec as AudioCodec, AudioTrackPlan};

/// `audio` — per-track policy for audio streams (DESIGN §6.2).
///
/// The **default policy** applies to tracks no rule names; **rules**
/// match tracks by codec and/or language and override it. Re-encoding
/// always applies to *all* matching tracks (deterministic target state).
/// **Atmos (E-AC-3 JOC) is copied unless an explicit rule re-encodes
/// it** — never auto-downmixed.
///
/// Flow JSON:
/// ```json
/// { "audio": {
///   "default": "copy",
///   "rules": [{
///     "match": { "codecs": ["dts", "dts_ma", "truehd"] },
///     "action": { "codec": "eac3", "sample_rate": 48000, "channels": 6 }
///   }]
/// } }
/// ```
///
/// v1 re-encode target codecs: see [`AudioCodec`].
/// The re-encode payload — the object form of [`AudioPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reencode {
    codec: AudioCodec,
    #[serde(default)]
    sample_rate: Option<u32>,
    #[serde(default)]
    channels: Option<u32>,
}

/// A policy for one or more tracks.
///
/// JSON: `"copy"` | `"drop"` | `{ "codec": "eac3", "sample_rate": 48000,
/// "channels": 6 }` (re-encode).
///
/// Manual serde: an untagged enum cannot represent its unit variants
/// as bare strings, so the documented `"copy"`/`"drop"` forms are
/// handled explicitly (case-insensitive).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AudioPolicy {
    /// Stream-copy (default — the design's "copy all").
    #[default]
    Copy,
    /// Drop the track(s).
    Drop,
    /// Re-encode to a target codec (all matching tracks, DESIGN §6.2).
    Reencode {
        codec: AudioCodec,
        sample_rate: Option<u32>,
        channels: Option<u32>,
    },
}

impl Serialize for AudioPolicy {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Copy => s.serialize_str("copy"),
            Self::Drop => s.serialize_str("drop"),
            Self::Reencode {
                codec,
                sample_rate,
                channels,
            } => Reencode {
                codec: *codec,
                sample_rate: *sample_rate,
                channels: *channels,
            }
            .serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AudioPolicy {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match v {
            Value::String(s) if s.eq_ignore_ascii_case("copy") => Ok(Self::Copy),
            Value::String(s) if s.eq_ignore_ascii_case("drop") => Ok(Self::Drop),
            // Legacy: the old UI wrote the string "re-encode" for the
            // re-encode policy. Map it to the default re-encode target
            // (E-AC-3, source rate/channels kept) so pre-existing flows
            // upgrade instead of failing at evaluation.
            Value::String(s) if s.eq_ignore_ascii_case("re-encode") => Ok(Self::Reencode {
                codec: AudioCodec::Eac3,
                sample_rate: None,
                channels: None,
            }),
            Value::Object(_) => serde_json::from_value::<Reencode>(v)
                .map(|r| Self::Reencode {
                    codec: r.codec,
                    sample_rate: r.sample_rate,
                    channels: r.channels,
                })
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "audio policy: expected \"copy\", \"drop\", or a re-encode object; got {other}"
            ))),
        }
    }
}

/// Which tracks a rule applies to. An empty `match` matches every track.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioMatch {
    /// Codec names (case-insensitive).
    #[serde(default)]
    pub codecs: Vec<String>,
    /// Language tags (e.g. `"eng"`, `"eng-2"`; case-insensitive).
    #[serde(default)]
    pub languages: Vec<String>,
}

impl AudioMatch {
    fn matches(&self, track: &crate::facts::AudioTrack) -> bool {
        let codec_ok = self.codecs.is_empty()
            || self
                .codecs
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&track.codec));
        let lang_ok = self.languages.is_empty()
            || track
                .language
                .as_deref()
                .is_some_and(|l| self.languages.iter().any(|w| w.eq_ignore_ascii_case(l)));
        codec_ok && lang_ok
    }
}

/// One per-track rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRule {
    #[serde(default, rename = "match")]
    pub matches: AudioMatch,
    pub action: AudioPolicy,
}

/// The `audio` section parameters.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOp {
    /// Policy for tracks no rule names (default `copy`).
    #[serde(default)]
    pub default: AudioPolicy,
    #[serde(default)]
    pub rules: Vec<AudioRule>,
}

/// The `audio` section entry.
pub struct Audio;

/// Choose the policy for one track: the first matching rule wins,
/// else the default.
fn choose(track: &crate::facts::AudioTrack, op: &AudioOp) -> (AudioPolicy, bool) {
    for rule in &op.rules {
        if rule.matches.matches(track) {
            return (rule.action.clone(), true);
        }
    }
    (op.default.clone(), false)
}

/// Plan every track. Returns the per-track decisions (index-aligned
/// with the source) — empty when everything is copied.
fn plan_tracks(op: &AudioOp, facts: &FileFacts) -> Vec<AudioTrackPlan> {
    let mut out = Vec::with_capacity(facts.audio.len());
    for track in &facts.audio {
        let (policy, explicit) = choose(track, op);
        // Atmos is never auto-downmixed (DESIGN §6.2): the *default*
        // re-encode copies it; only an explicit rule re-encodes it.
        if track.atmos
            && !explicit
            && matches!(
                policy,
                AudioPolicy::Reencode {
                    codec: AudioCodec::Eac3,
                    ..
                } | AudioPolicy::Reencode {
                    codec: AudioCodec::Ac3,
                    ..
                } | AudioPolicy::Reencode {
                    codec: AudioCodec::Aac,
                    ..
                }
            )
        {
            out.push(AudioTrackPlan::Copy);
            continue;
        }
        match policy {
            AudioPolicy::Copy => out.push(AudioTrackPlan::Copy),
            AudioPolicy::Drop => out.push(AudioTrackPlan::Drop),
            AudioPolicy::Reencode {
                codec,
                sample_rate,
                channels,
            } => {
                // If the target is the source codec, nothing changes.
                let same_codec = match &codec {
                    AudioCodec::Eac3 => {
                        track.codec.eq_ignore_ascii_case("eac3")
                            || track.codec.eq_ignore_ascii_case("eac3_joc")
                    }
                    AudioCodec::Ac3 => track.codec.eq_ignore_ascii_case("ac3"),
                    AudioCodec::Aac => track.codec.eq_ignore_ascii_case("aac"),
                };
                if same_codec
                    && sample_rate.is_none_or(|r| track.sample_rate == Some(r))
                    && channels.is_none_or(|c| track.channels == Some(c))
                {
                    out.push(AudioTrackPlan::Copy);
                } else {
                    out.push(AudioTrackPlan::Reencode {
                        codec,
                        sample_rate,
                        channels: channels.or(track.channels),
                        bitrate_bps: Some(codec.default_bitrate_bps(channels.or(track.channels))),
                    });
                }
            }
        }
    }
    out
}

impl OperationSection for Audio {
    fn key(&self) -> &'static str {
        "audio"
    }

    fn description(&self) -> &'static str {
        "Audio tracks: default policy + per-track rules (Atmos never auto-downmixed)"
    }

    fn plan(&self, params: &Value, facts: &FileFacts) -> crate::error::Result<SectionPlan> {
        let op: AudioOp = parse(self.key(), params)?;
        let plans = plan_tracks(&op, facts);
        if plans.iter().all(|p| matches!(p, AudioTrackPlan::Copy)) {
            return Ok(SectionPlan::Identity);
        }
        Ok(SectionPlan::Audio(AudioPlan { per_track: plans }))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "object",
            "fields": {
                "default": audio_policy_schema(),
                "rules": {
                    "kind": "list",
                    "item": {
                        "match": { "codecs": { "kind": "multi_select", "values": crate::registry::condition::audio_codec::AUDIO_CODECS }, "languages": { "kind": "multi_select" } },
                        "action": audio_policy_schema()
                    },
                    "hint": "First matching rule wins. Re-encode applies to ALL matching tracks."
                }
            }
        })
    }
}

/// The `audio_policy` ui_schema fragment — the single source for the
/// copy/drop/re-encode vocabulary and the re-encode target codecs, so the
/// frontend renders from the schema (a new codec is a Rust-only change).
pub fn audio_policy_schema() -> Value {
    json!({
        "kind": "audio_policy",
        "default": "copy",
        "values": [
            { "value": "copy", "label": "Copy" },
            { "value": "drop", "label": "Drop" },
            { "value": "re_encode", "label": "Re-encode" }
        ],
        "reencode": {
            "codec": {
                "kind": "single_select",
                "default": "eac3",
                "values": [
                    { "value": "eac3", "label": "E-AC-3" },
                    { "value": "ac3", "label": "AC-3" },
                    { "value": "aac", "label": "AAC" }
                ]
            },
            "sample_rate": { "kind": "text", "hint": "Hertz, e.g. 48000. Leave empty to keep the source rate." },
            "channels": { "kind": "text", "hint": "Leave empty to keep the source channel count." }
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::AudioTrack;

    fn track(codec: &str, atmos: bool) -> AudioTrack {
        AudioTrack {
            index: 0,
            codec: codec.into(),
            channels: Some(6),
            atmos,
            ..Default::default()
        }
    }

    fn facts(tracks: Vec<AudioTrack>) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            audio: tracks,
            ..Default::default()
        }
    }

    #[test]
    fn audio_policy_json_forms() {
        let p: AudioPolicy = serde_json::from_str("\"copy\"").unwrap();
        assert!(matches!(p, AudioPolicy::Copy));
        let p: AudioPolicy = serde_json::from_str("\"drop\"").unwrap();
        assert!(matches!(p, AudioPolicy::Drop));
        let p: AudioPolicy =
            serde_json::from_str(r#"{ "codec": "ac3", "sample_rate": 48000 }"#).unwrap();
        assert!(matches!(
            p,
            AudioPolicy::Reencode {
                codec: AudioCodec::Ac3,
                sample_rate: Some(48_000),
                channels: None
            }
        ));
        // Legacy: the old UI wrote the string "re-encode" — it upgrades to
        // the default re-encode target (E-AC-3, source rate/channels kept).
        let p: AudioPolicy = serde_json::from_str("\"re-encode\"").unwrap();
        assert!(matches!(
            p,
            AudioPolicy::Reencode {
                codec: AudioCodec::Eac3,
                sample_rate: None,
                channels: None
            }
        ));
        assert!(serde_json::from_str::<AudioPolicy>("\"reencode\"").is_err());
    }

    #[test]
    fn default_copy_is_identity() {
        let a = Audio;
        let f = facts(vec![track("dts", false), track("truehd", true)]);
        let p = a.plan(&json!({}), &f).unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn rule_reencodes_matching_tracks_only() {
        let a = Audio;
        let f = facts(vec![
            track("eac3", false),
            track("dts", false),
            track("truehd", false),
        ]);
        let p = a
            .plan(
                &json!({
                    "rules": [{
                        "match": { "codecs": ["dts", "truehd"] },
                        "action": { "codec": "eac3", "sample_rate": 48000, "channels": 6 }
                    }]
                }),
                &f,
            )
            .unwrap();
        let AudioPlan { per_track } = match p {
            SectionPlan::Audio(p) => p,
            other => panic!("expected audio plan, got {other:?}"),
        };
        assert!(matches!(per_track[0], AudioTrackPlan::Copy));
        assert!(matches!(
            per_track[1],
            AudioTrackPlan::Reencode {
                codec: AudioCodec::Eac3,
                sample_rate: Some(48000),
                ..
            }
        ));
        assert!(matches!(per_track[2], AudioTrackPlan::Reencode { .. }));
    }

    #[test]
    fn atmos_is_copied_under_default_reencode() {
        let a = Audio;
        let f = facts(vec![track("eac3_joc", true)]);
        let p = a
            .plan(
                &json!({
                    "default": { "codec": "aac", "channels": 2 }
                }),
                &f,
            )
            .unwrap();
        assert!(
            matches!(p, SectionPlan::Identity),
            "default re-encode must not touch Atmos"
        );
    }

    #[test]
    fn atmos_reencoded_by_explicit_rule() {
        let a = Audio;
        let f = facts(vec![track("eac3_joc", true)]);
        let p = a
            .plan(
                &json!({
                    "rules": [{
                        "match": { "codecs": ["eac3_joc"] },
                        "action": { "codec": "eac3", "sample_rate": 48000 }
                    }]
                }),
                &f,
            )
            .unwrap();
        assert!(!matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn drop_removes_tracks() {
        let a = Audio;
        let f = facts(vec![track("eac3", false), track("dts", false)]);
        let p = a
            .plan(
                &json!({ "rules": [{ "match": { "codecs": ["dts"] }, "action": "drop" }] }),
                &f,
            )
            .unwrap();
        let AudioPlan { per_track } = match p {
            SectionPlan::Audio(p) => p,
            other => panic!("expected audio plan, got {other:?}"),
        };
        assert!(matches!(per_track[0], AudioTrackPlan::Copy));
        assert!(matches!(per_track[1], AudioTrackPlan::Drop));
    }

    #[test]
    fn policy_strings_round_trip() {
        let s = serde_json::to_string(&AudioPolicy::Copy).unwrap();
        assert_eq!(s, "\"copy\"");
        let d: AudioPolicy = serde_json::from_str("\"DROP\"").unwrap();
        assert!(matches!(d, AudioPolicy::Drop));
        let r: AudioPolicy = serde_json::from_str("{\"codec\":\"aac\",\"channels\":2}").unwrap();
        assert!(matches!(
            r,
            AudioPolicy::Reencode {
                codec: AudioCodec::Aac,
                channels: Some(2),
                ..
            }
        ));
        let bad = serde_json::from_str::<AudioPolicy>("\"keep\"");
        assert!(bad.is_err());
    }
}
