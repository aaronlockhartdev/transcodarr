use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::plan::{SubtitlePlan, SubtitlePolicy};
use crate::registry::operation::parse;
use crate::registry::{OperationSection, SectionPlan};

/// `subtitles` — what happens to subtitle tracks (DESIGN §6.4).
///
/// Copy only — no burn-in in v1 (parked, §12).
///
/// Flow JSON: `{ "subtitles": { "policy": "keep_all" | "keep_first" | "keep_forced" | "drop" } }`.
/// Absent section = `keep_all`.
pub struct Subtitles;

/// The `subtitles` section parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitlesOp {
    #[serde(default)]
    pub policy: SubtitlePolicy,
}

impl OperationSection for Subtitles {
    fn key(&self) -> &'static str {
        "subtitles"
    }

    fn description(&self) -> &'static str {
        "Subtitle tracks: keep all, keep the first, keep forced-only, or drop (copy only)"
    }

    fn plan(&self, params: &Value, facts: &FileFacts) -> crate::error::Result<SectionPlan> {
        let op: SubtitlesOp = parse(self.key(), params)?;
        let tracks = match op.policy {
            SubtitlePolicy::KeepAll => facts.subtitles.clone(),
            // The first track in file order; files without subtitles
            // yield an empty survivor list (the identity check below
            // turns that into a no-op plan).
            SubtitlePolicy::KeepFirst => facts.subtitles.first().cloned().into_iter().collect(),
            SubtitlePolicy::KeepForced => facts
                .subtitles
                .iter()
                .filter(|s| s.forced)
                .cloned()
                .collect(),
            SubtitlePolicy::Drop => Vec::new(),
        };
        // Identity when nothing is dropped.
        if tracks.len() == facts.subtitles.len() {
            return Ok(SectionPlan::Identity);
        }
        Ok(SectionPlan::Subtitles(SubtitlePlan {
            policy: op.policy,
            tracks,
        }))
    }

    fn validate(&self, params: &Value) -> crate::error::Result<()> {
        parse::<SubtitlesOp>(self.key(), params).map(|_| ())
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "object",
            "label": "Subtitles",
            "fields": {
                "policy": {
                    "kind": "single_select",
                    "label": "Policy",
                    "values": [
                        { "value": "keep_all", "label": "Keep all" },
                        { "value": "keep_first", "label": "Keep first" },
                        { "value": "keep_forced", "label": "Keep forced only" },
                        { "value": "drop", "label": "Drop" }
                    ],
                    "default": "keep_all",
                    "hint": "Subtitles are copied, never re-encoded."
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::SubtitleTrack;

    fn facts(subs: &[bool]) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            subtitles: subs
                .iter()
                .enumerate()
                .map(|(i, f)| SubtitleTrack {
                    index: i as u32,
                    codec: "srt".into(),
                    forced: *f,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn keep_all_is_identity() {
        let s = Subtitles;
        let p = s
            .plan(&json!({ "policy": "keep_all" }), &facts(&[true, false]))
            .unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn keep_forced_is_identity_when_all_forced() {
        let s = Subtitles;
        let p = s
            .plan(&json!({ "policy": "keep_forced" }), &facts(&[true, true]))
            .unwrap();
        assert!(matches!(p, SectionPlan::Identity));
        let p = s
            .plan(&json!({ "policy": "keep_forced" }), &facts(&[true, false]))
            .unwrap();
        assert!(!matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn keep_forced_lists_survivors() {
        let s = Subtitles;
        let p = s
            .plan(
                &json!({ "policy": "keep_forced" }),
                &facts(&[true, false, true]),
            )
            .unwrap();
        let SectionPlan::Subtitles(sp) = p else {
            panic!("{p:?}");
        };
        assert_eq!(
            sp.tracks.iter().map(|t| t.index).collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert!(sp.changes_anything());
    }

    #[test]
    fn drop_is_identity_without_subs() {
        let s = Subtitles;
        let p = s.plan(&json!({ "policy": "drop" }), &facts(&[])).unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn drop_everything_plans_null_subtitles() {
        let s = Subtitles;
        let p = s
            .plan(&json!({ "policy": "drop" }), &facts(&[false]))
            .unwrap();
        let SectionPlan::Subtitles(sp) = p else {
            panic!("{p:?}");
        };
        assert!(sp.tracks.is_empty());
    }

    #[test]
    fn default_policy_is_keep_all() {
        let s = Subtitles;
        let p = s.plan(&json!({}), &facts(&[false])).unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn keep_first_is_a_noop_without_subtitles_or_with_one() {
        let s = Subtitles;
        let p = s
            .plan(&json!({ "policy": "keep_first" }), &facts(&[]))
            .unwrap();
        assert!(matches!(p, SectionPlan::Identity));
        // A single track is kept whole: nothing changes.
        let p = s
            .plan(&json!({ "policy": "keep_first" }), &facts(&[false]))
            .unwrap();
        assert!(matches!(p, SectionPlan::Identity));
    }

    #[test]
    fn keep_first_keeps_only_the_first_track() {
        let s = Subtitles;
        let p = s
            .plan(
                &json!({ "policy": "keep_first" }),
                &facts(&[true, false, true]),
            )
            .unwrap();
        let SectionPlan::Subtitles(sp) = p else {
            panic!("{p:?}");
        };
        assert_eq!(
            sp.tracks.iter().map(|t| t.index).collect::<Vec<_>>(),
            vec![0]
        );
        assert!(sp.changes_anything());
    }

    #[test]
    fn policy_wire_round_trips() {
        let s = serde_json::to_string(&SubtitlePolicy::KeepFirst).unwrap();
        assert_eq!(s, "\"keep_first\"");
        let p: SubtitlePolicy = serde_json::from_str("\"keep_first\"").unwrap();
        assert!(matches!(p, SubtitlePolicy::KeepFirst));
    }
}
