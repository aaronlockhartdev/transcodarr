use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::{FileFacts, Hdr};
use crate::registry::ConditionField;
use crate::registry::condition::parse;

/// `hdr` — matches the file's HDR metadata.
///
/// Flow JSON: `{ "hdr": { "in": ["hdr10"] } }`. Values: `none` (SDR),
/// `hdr10`, `hdr10_plus`, `dolby_vision`, `hlg`. Files without video
/// never match a non-"any" constraint.
///
/// (Named `HdrField` to avoid clashing with [`crate::facts::Hdr`].)
pub struct HdrField;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    #[serde(default)]
    r#in: Vec<Hdr>,
}

impl ConditionField for HdrField {
    fn key(&self) -> &'static str {
        "hdr"
    }

    fn description(&self) -> &'static str {
        "HDR metadata (SDR, HDR10, HDR10+, Dolby Vision, HLG)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.r#in.is_empty() {
            return Ok(true);
        }
        let Some(v) = facts.video() else {
            return Ok(false);
        };
        Ok(c.r#in.contains(&v.hdr))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "multi_select",
            "values": ["none", "hdr10", "hdr10_plus", "dolby_vision", "hlg"],
            "hint": "`none` means SDR. Leave empty for any.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::VideoFacts;

    fn facts(hdr: Hdr) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            video: Some(VideoFacts {
                codec: "hevc".into(),
                width: 3840,
                height: 2160,
                hdr,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn matches() {
        let c = HdrField;
        let v = json!({ "in": ["hdr10", "none"] });
        assert!(c.match_facts(&v, &facts(Hdr::Hdr10)).unwrap());
        assert!(c.match_facts(&v, &facts(Hdr::None)).unwrap());
        assert!(!c.match_facts(&v, &facts(Hdr::Hlg)).unwrap());
    }
}
