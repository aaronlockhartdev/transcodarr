use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::registry::operation::video::{ContainerChoice, VideoOp};

/// A step's operation: the **complete** plan for a matched file
/// (DESIGN §6).
///
/// Each entry's key is an operation section's `key` (see
/// [`crate::registry::OperationSection`]) and its value is the section's
/// own JSON parameters. An **absent** section is identity for its domain
/// (everything copied/kept); nothing outside the flow is ever touched.
///
/// `container` is special: it changes no stream, it changes the
/// wrapper. It may be written as a top-level operation field
/// (DESIGN §6.3) or nested inside the `video` section (the §6 worked
/// example); top-level wins. Absent ⇒ `smart`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// Target container (DESIGN §6.3). Top-level form; a value nested in
    /// the `video` section is also accepted (top-level wins).
    /// Absent ⇒ `smart`.
    #[serde(default)]
    pub container: Option<ContainerChoice>,
    /// Per-section parameters, keyed by section key.
    #[serde(flatten)]
    pub sections: BTreeMap<String, Value>,
}

impl Operation {
    /// True when no section is present (pure identity step).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// The effective target container: top-level field, else the
    /// `video` section's `container`, else `smart`.
    #[must_use]
    pub fn container(&self) -> ContainerChoice {
        if let Some(c) = &self.container {
            return *c;
        }
        let video_json = self.sections.get("video");
        let container = video_json.and_then(|v| v.get("container")).or_else(|| {
            video_json
                .and_then(|v| v.get("video"))
                .and_then(|v| v.get("container"))
        });
        match container {
            Some(Value::String(s)) => match s.as_str() {
                "smart" => ContainerChoice::Smart,
                "mp4" => ContainerChoice::Mp4,
                "mkv" => ContainerChoice::Mkv,
                other => {
                    // Unknown value: default to smart (a schema/UI
                    // concern, not a flow error — the section itself
                    // validates it when planning).
                    let _ = other;
                    ContainerChoice::Smart
                }
            },
            _ => ContainerChoice::Smart,
        }
    }

    /// The `video` section's operation (the only section with a typed
    /// view), if present.
    #[must_use]
    pub fn video(&self) -> Option<VideoOp> {
        let v = self.sections.get("video")?;
        // Accept a bare object; unwrap a {"video": {...}} envelope.
        let v = v.get("video").unwrap_or(v);
        serde_json::from_value(v.clone()).ok()
    }
}
