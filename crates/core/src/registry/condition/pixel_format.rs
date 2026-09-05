use serde::Deserialize;
use serde_json::{json, Value};

use crate::facts::FileFacts;
use crate::registry::condition::parse;
use crate::registry::ConditionField;

/// `pixel_format` — matches the file's video pixel format.
///
/// Flow JSON: `{ "pixel_format": { "in": ["yuv420p10le"] } }` (10-bit
/// detection for profile decisions). Files without video never match a
/// non-"any" constraint.
pub struct PixelFormat;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    #[serde(default)]
    r#in: Vec<String>,
}

impl ConditionField for PixelFormat {
    fn key(&self) -> &'static str {
        "pixel_format"
    }

    fn description(&self) -> &'static str {
        "Video pixel format / bit depth (yuv420p, yuv420p10le, …)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.r#in.is_empty() {
            return Ok(true);
        }
        let Some(v) = facts.video() else {
            return Ok(false);
        };
        let Some(actual) = v.pixel_format.as_deref() else {
            return Ok(false);
        };
        Ok(c.r#in.iter().any(|w| w.eq_ignore_ascii_case(actual)))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "multi_select",
            "values": ["yuv420p", "yuv420p10le", "yuv422p10le", "yuv444p10le", "yuvj420p"],
            "hint": "…10le variants are 10-bit (drives HEVC Main10 and friends).",
        })
    }
}
