//! Flow types: the versioned JSON document a library runs against.
//!
//! A flow is **data** (DESIGN §5): its variable parts are registry
//! entries, referenced by key from the JSON. This module holds the
//! stable envelope (`Flow`, `FlowStep`); the per-field and per-section
//! JSON is owned by the registry entries it points to.

pub mod condition;
pub mod operation;

pub use condition::Condition;
pub use operation::Operation;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A library's format policy (DESIGN §2, §5).
///
/// Serialized with `flow_version` for forward compatibility; the
/// registry (not this type) decides what a given document means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    /// Flow schema version. Must match what this build supports.
    pub flow_version: u32,
    /// Display name (UI only).
    #[serde(default)]
    pub name: Option<String>,
    /// Ordered steps; **first match wins** (DESIGN §2).
    pub steps: Vec<FlowStep>,
    /// Behavior when no step matches (DESIGN §2).
    #[serde(default)]
    pub no_match: NoMatchPolicy,
}

/// What happens when no step's condition matches a file: the file is
/// left untouched with status `unmatched`, optionally escalated to a
/// warning (a no-match is the classic flow-authoring bug and must
/// never be silent — DESIGN §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NoMatchPolicy {
    /// Surface unmatched files as warnings in the UI.
    #[serde(default)]
    pub escalate: bool,
}

impl Flow {
    /// A copy of this flow with every user filter graph cleared
    /// (DESIGN §13.6 / §6.5).
    ///
    /// The post-job idempotency gate re-evaluates the *new* file:
    /// filters are one-shot transformations that facts cannot reflect
    /// (a filtered H.264 is still H.264), so without this they would
    /// make every filtered job fail `non_idempotent` and every scan
    /// re-queue the file forever.
    #[must_use]
    pub fn with_filters_cleared(&self) -> Self {
        let mut copy = self.clone();
        for step in &mut copy.steps {
            clear_filters_in(&mut step.operation);
        }
        copy
    }
}

/// Remove `video_filter`/`audio_filter` keys from the video/audio
/// section parameters of one operation (both the bare form and the
/// lenient `{ "video": { … } }` envelope).
fn clear_filters_in(op: &mut Operation) {
    for key in ["video", "audio"] {
        if let Some(v) = op.sections.get_mut(key) {
            if let Some(m) = v.as_object_mut() {
                m.remove("video_filter");
                m.remove("audio_filter");
                if let Some(inner) = m.get_mut(key).and_then(Value::as_object_mut) {
                    inner.remove("video_filter");
                    inner.remove("audio_filter");
                }
            }
        }
    }
}

/// One step: `condition → operation` (DESIGN §2).
///
/// The condition is an AND of per-field constraints; the operation is
/// the **complete** plan for a matched file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowStep {
    /// Optional stable id, preserved verbatim (nothing assigns one; the UI
    /// orders steps by array position, not this field).
    #[serde(default)]
    pub id: String,
    /// Optional user-assigned display name (UI only, like `id` — preserved
    /// verbatim). Unnamed steps render as "Step N" by array position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// AND of per-field constraints; absent fields are "any".
    ///
    /// An empty condition matches **every** file — a catch-all step.
    #[serde(default)]
    pub condition: Condition,
    /// The complete transformation. Absent sections are identity.
    #[serde(default)]
    pub operation: Operation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_name_is_optional_and_preserved() {
        let f: Flow = serde_json::from_str(
            r#"{"flow_version":1,"steps":[{"condition":{},"operation":{},"name":"4K to 1080p"}]}"#,
        )
        .unwrap();
        assert_eq!(f.steps[0].name.as_deref(), Some("4K to 1080p"));

        // An unnamed step round-trips without a `name` key at all.
        let g: Flow =
            serde_json::from_str(r#"{"flow_version":1,"steps":[{"condition":{},"operation":{}}]}"#)
                .unwrap();
        assert_eq!(g.steps[0].name, None);
        let v = serde_json::to_value(&g).unwrap();
        assert!(v["steps"][0].get("name").is_none());
    }

    #[test]
    fn with_filters_cleared_strips_graphs_from_all_sections() {
        let doc = r#"{"flow_version":1,"steps":[
            {"condition":{"container":{"in":["mkv"]}},
             "operation":{"video":{"codec":"h264","video_filter":"crop=10:10"},"audio":{"default":"copy","audio_filter":"volume=2"}}},
            {"condition":{},
             "operation":{"video":{"video":{"codec":"hevc","video_filter":"denoise"}},"audio":{"default":"copy"}}}
        ]}"#;
        let f: Flow = serde_json::from_str(doc).unwrap();
        let c = f.with_filters_cleared();
        // Bare form.
        let v1 = c.steps[0].operation.sections["video"].clone();
        assert!(v1.get("video_filter").is_none());
        let a1 = c.steps[0].operation.sections["audio"].clone();
        assert!(a1.get("audio_filter").is_none());
        // Lenient envelope form.
        let v2 = c.steps[1].operation.sections["video"].clone();
        assert!(v2["video"].get("video_filter").is_none());
        // Everything else is untouched.
        assert_eq!(v1["codec"], "h264");
        assert_eq!(v2["video"]["codec"], "hevc");
        assert_eq!(
            c.steps[0]
                .condition
                .fields
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["container".to_string()]
        );
        // The original is not modified.
        assert!(
            f.steps[0].operation.sections["video"]
                .get("video_filter")
                .is_some()
        );
    }
}
