//! Evaluation: the pure `(flow, facts) → plan` function (DESIGN §2, §7).
//!
//! `evaluate` is the entire decision core: given a library's flow and a
//! file's cached facts, it returns
//!
//! - [`Evaluation::Identity`] — the matched step's operation would
//!   change nothing (a compliant file — stream-copy only, same
//!   container);
//! - [`Evaluation::Plan`] — a complete, device-independent
//!   [`FfmpegPlan`] for one output file;
//! - [`Evaluation::NoMatch`] — no step's condition matched (the file is
//!   left untouched with status `unmatched`; a no-match must never be
//!   silent, DESIGN §2).
//!
//! First match wins (DESIGN §2). This function is **pure** — it never
//! touches the filesystem or a probe (facts are cached inputs), which is
//! what makes re-evaluation after a flow edit cheap and testable.

use crate::error::CoreError;
use crate::facts::FileFacts;
use crate::flow::{Condition, Flow, Operation};
use crate::plan::{FfmpegPlan, VideoPlan};
use crate::registry::{Registry, SectionPlan};

/// The flow schema version this build implements (DESIGN §2, §11).
pub const FLOW_VERSION: u32 = 1;

/// The result of evaluating one file against a flow.
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluation {
    /// The file is compliant: the matched operation would change
    /// nothing.
    Identity,
    /// A complete plan: exactly one output file for the input.
    Plan(FfmpegPlan),
    /// No step matched; the file is left untouched (status `unmatched`).
    NoMatch,
}

impl Evaluation {
    /// Whether this evaluation leaves the file untouched.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        matches!(self, Self::Identity | Self::NoMatch)
    }
}

/// Evaluate `facts` against `flow` using `registry` (DESIGN §2, §7).
///
/// This is pure and synchronous; the caller owns caching facts and
/// recording the result (the server persists `plan_json` and
/// `status`).
pub fn evaluate(
    registry: &Registry,
    flow: &Flow,
    facts: &FileFacts,
) -> crate::error::Result<Evaluation> {
    if flow.flow_version != FLOW_VERSION {
        return Err(CoreError::UnsupportedFlowVersion(flow.flow_version));
    }
    for step in &flow.steps {
        if !matches_condition(registry, &step.condition, facts)? {
            continue;
        }
        // First match wins: this step's operation is the complete plan.
        let plan = plan_operation(registry, &step.operation, facts)?;
        if plan.changes_anything() {
            return Ok(Evaluation::Plan(plan));
        }
        return Ok(Evaluation::Identity);
    }
    Ok(Evaluation::NoMatch)
}

/// AND-combine every field constraint of a condition (DESIGN §2, §5).
/// Unknown field keys are flow errors (surfaced, not ignored).
fn matches_condition(
    registry: &Registry,
    condition: &Condition,
    facts: &FileFacts,
) -> crate::error::Result<bool> {
    for (key, constraint) in &condition.fields {
        let field = registry
            .condition_field(key)
            .ok_or_else(|| CoreError::UnknownConditionField(key.clone()))?;
        if !field.match_facts(constraint, facts)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Build the plan for a matched operation (DESIGN §6, §7).
///
/// Every section is planned independently; the container is resolved
/// from the operation's `container` (or the smart default when the
/// operation is present but says nothing about it). An **empty**
/// operation is identity — the file is not even re-muxed.
fn plan_operation(
    registry: &Registry,
    operation: &Operation,
    facts: &FileFacts,
) -> crate::error::Result<FfmpegPlan> {
    let mut video = VideoPlan::Copy;
    let mut audio: Option<crate::plan::AudioPlan> = None;
    let mut subtitles: Option<crate::plan::SubtitlePlan> = None;

    for (key, params) in &operation.sections {
        let section = registry
            .operation_section(key)
            .ok_or_else(|| CoreError::UnknownOperationSection(key.clone()))?;
        match section.plan(params, facts)? {
            SectionPlan::Identity => {}
            SectionPlan::Video(v) => video = v,
            SectionPlan::Audio(a) => audio = Some(a),
            SectionPlan::Subtitles(s) => subtitles = Some(s),
        }
    }

    // Container: absent ⇒ smart. An entirely empty operation changes
    // nothing at all — not even the container (a compliant file stays
    // byte-identical, DESIGN §2).
    let container = if operation.is_empty() {
        facts.container.clone()
    } else {
        let choice = operation.container();
        crate::registry::operation::container::resolve(choice, facts, &audio).to_string()
    };
    let remux = !facts.container.eq_ignore_ascii_case(&container);

    // An audio plan that only copies is the same as no audio plan.
    let audio = audio.filter(|a| a.changes_anything());

    Ok(FfmpegPlan {
        container,
        video,
        audio,
        subtitles,
        remux,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::facts::{AudioTrack, VideoFacts};
    use crate::flow::FlowStep;
    use crate::registry::Registry;

    fn registry() -> Registry {
        Registry::v1()
    }

    fn flow(steps: Vec<(BTreeMap<String, serde_json::Value>, serde_json::Value)>) -> Flow {
        Flow {
            flow_version: FLOW_VERSION,
            name: None,
            steps: steps
                .into_iter()
                .map(|(c, o)| FlowStep {
                    id: "s".into(),
                    condition: Condition { fields: c },
                    operation: serde_json::from_value(o).unwrap(),
                })
                .collect(),
            no_match: Default::default(),
        }
    }

    fn h264_1080p_facts() -> FileFacts {
        FileFacts {
            container: "mp4".into(),
            video: Some(VideoFacts {
                codec: "h264".into(),
                profile: Some("high".into()),
                level: Some("4.2".into()),
                width: 1920,
                height: 1080,
                bitrate_bps: Some(8_000_000),
                ..Default::default()
            }),
            audio: vec![AudioTrack {
                codec: "eac3".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn compliant_file_is_identity() {
        let r = registry();
        // 1080p H.264 already meets the 1080p H.264 target.
        let f = flow(vec![(
            BTreeMap::new(),
            json!({
                "video": {
                    "codec": "h264",
                    "profile": "high",
                    "level": "4.2",
                    "bitrate": { "mode": "source_capped" }
                }
            }),
        )]);
        let e = evaluate(&r, &f, &h264_1080p_facts()).unwrap();
        assert!(matches!(e, Evaluation::Identity), "{e:?}");
    }

    #[test]
    fn first_match_wins() {
        let r = registry();
        let f = flow(vec![
            (
                BTreeMap::from([("container".to_string(), json!({ "in": ["mkv"] }))]),
                json!({ "video": { "codec": "h264", "downscale_to": [1280, 720] } }),
            ),
            (
                BTreeMap::new(),
                json!({ "video": { "codec": "hevc" } }),
            ),
        ]);
        let mut facts = h264_1080p_facts();
        facts.container = "mkv".into();
        let e = evaluate(&r, &f, &facts).unwrap();
        // The first (mkv) step matches: h264 720p, not hevc.
        let Evaluation::Plan(p) = &e else {
            panic!("expected plan: {e:?}");
        };
        let VideoPlan::Encode { codec, target_width, .. } = &p.video else {
            panic!("expected encode: {p:?}");
        };
        assert_eq!(*codec, crate::plan::VideoTargetCodec::H264);
        assert_eq!(*target_width, 1280);
    }

    #[test]
    fn no_match_is_reported() {
        let r = registry();
        let f = flow(vec![(
            BTreeMap::from([("container".to_string(), json!({ "in": ["mkv"] }))]),
            json!({ "video": { "codec": "hevc" } }),
        )]);
        let e = evaluate(&r, &f, &h264_1080p_facts()).unwrap();
        assert!(matches!(e, Evaluation::NoMatch));
    }

    #[test]
    fn unknown_field_is_an_error_not_a_silent_match() {
        let r = registry();
        let f = flow(vec![(
            BTreeMap::from([("does_not_exist".to_string(), json!("x"))]),
            json!({ "video": { "codec": "hevc" } }),
        )]);
        let e = evaluate(&r, &f, &h264_1080p_facts());
        assert!(matches!(
            e,
            Err(CoreError::UnknownConditionField(_))
        ));
    }

    #[test]
    fn unsupported_flow_version_is_an_error() {
        let r = registry();
        let mut f = flow(vec![]);
        f.flow_version = 99;
        assert!(matches!(
            evaluate(&r, &f, &h264_1080p_facts()),
            Err(CoreError::UnsupportedFlowVersion(99))
        ));
    }

    #[test]
    fn hdr_constraint_matches_only_listed_hdr() {
        let r = registry();
        let f = flow(vec![(
            BTreeMap::from([("hdr".to_string(), json!({ "in": ["hdr10"] }))]),
            json!({ "video": { "codec": "h264", "hdr_to_sdr": true } }),
        )]);
        // SDR file must not match a hdr10 constraint.
        let e = evaluate(&r, &f, &h264_1080p_facts()).unwrap();
        assert!(matches!(e, Evaluation::NoMatch), "{e:?}");
    }

    #[test]
    fn audio_reencode_produces_per_track_plan() {
        let r = registry();
        let f = flow(vec![(
            BTreeMap::new(),
            json!({
                "audio": {
                    "rules": [{
                        "match": { "codecs": ["eac3"] },
                        "action": { "codec": "ac3", "sample_rate": 48000, "channels": 2 }
                    }]
                }
            }),
        )]);
        let e = evaluate(&r, &f, &h264_1080p_facts()).unwrap();
        let Evaluation::Plan(p) = &e else {
            panic!("expected plan: {e:?}");
        };
        let Some(a) = &p.audio else {
            panic!("expected audio plan: {p:?}");
        };
        assert_eq!(a.per_track.len(), 1);
        assert!(matches!(
            a.per_track[0],
            crate::plan::AudioTrackPlan::Reencode {
                codec: crate::plan::AudioTargetCodec::Ac3,
                ..
            }
        ));
        // eac3→ac3: container stays smart → mp4 (both are MP4-safe).
        assert_eq!(p.container, "mp4");
    }
}
