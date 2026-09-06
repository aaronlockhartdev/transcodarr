use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::registry::ConditionField;
use crate::registry::condition::parse;

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
            "label": "Pixel format",
            "values": [
                { "value": "yuv420p", "label": "8-bit 4:2:0" },
                { "value": "yuv420p10le", "label": "10-bit 4:2:0" },
                { "value": "yuv422p10le", "label": "10-bit 4:2:2" },
                { "value": "yuv444p10le", "label": "10-bit 4:4:4" },
                { "value": "yuvj420p", "label": "8-bit 4:2:0 (JPEG)" }
            ],
            "hint": "The file's video must use one of these pixel formats. Leave empty for any."
        })
    }
}
