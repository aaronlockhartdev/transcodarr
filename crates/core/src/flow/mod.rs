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
}
