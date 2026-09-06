use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::registry::ConditionField;
use crate::registry::condition::parse;

/// `container` — matches the file's container format.
///
/// Flow JSON: `{ "container": { "in": ["mp4", "mkv"] } }`. Matches when
/// the file's container (case-insensitive) is in the list; empty/absent
/// means "any".
pub struct Container;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    #[serde(default)]
    r#in: Vec<String>,
}

impl ConditionField for Container {
    fn key(&self) -> &'static str {
        "container"
    }

    fn description(&self) -> &'static str {
        "Container format of the file (mkv, mp4, …)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.r#in.is_empty() {
            return Ok(true);
        }
        let actual = facts.container.to_lowercase();
        Ok(c.r#in.iter().any(|w| w.eq_ignore_ascii_case(&actual)))
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "multi_select",
            "label": "Container",
            "values": [
                { "value": "mkv", "label": "MKV" },
                { "value": "mp4", "label": "MP4" },
                { "value": "mov", "label": "MOV" },
                { "value": "m2ts", "label": "MPEG-TS" },
                { "value": "webm", "label": "WebM" },
                { "value": "avi", "label": "AVI" }
            ],
            "hint": "The file must be in one of these containers. Leave empty for any."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(container: &str) -> FileFacts {
        FileFacts {
            container: container.into(),
            ..Default::default()
        }
    }

    #[test]
    fn matches_listed_container() {
        let c = Container;
        let v = json!({ "in": ["mp4", "mkv"] });
        assert!(c.match_facts(&v, &facts("mkv")).unwrap());
        assert!(!c.match_facts(&v, &facts("mov")).unwrap());
    }

    #[test]
    fn is_case_insensitive() {
        let c = Container;
        let v = json!({ "in": ["MKV"] });
        assert!(c.match_facts(&v, &facts("mkv")).unwrap());
    }

    #[test]
    fn empty_or_absent_is_any() {
        let c = Container;
        assert!(c.match_facts(&json!({}), &facts("whatever")).unwrap());
        assert!(
            c.match_facts(&json!({ "in": [] }), &facts("whatever"))
                .unwrap()
        );
    }

    #[test]
    fn rejects_malformed_constraint() {
        let c = Container;
        assert!(c.match_facts(&json!({ "in": 5 }), &facts("mkv")).is_err());
    }
}
