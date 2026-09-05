use serde::Deserialize;
use serde_json::{json, Value};

use crate::facts::FileFacts;
use crate::registry::condition::parse;
use crate::registry::ConditionField;

/// `file_size` — matches the file size in bytes.
///
/// Flow JSON: `{ "file_size": { "min": 1073741824, "max": 21474836480 } }`
/// (inclusive; absent bound = unbounded). The UI renders the bound in
/// MB/GB from the schema's `unit`.
pub struct FileSize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    min: Option<u64>,
    max: Option<u64>,
}

impl ConditionField for FileSize {
    fn key(&self) -> &'static str {
        "file_size"
    }

    fn description(&self) -> &'static str {
        "File size in bytes (min/max, inclusive)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.min.is_none() && c.max.is_none() {
            return Ok(true);
        }
        if let Some(min) = c.min {
            if facts.size < min {
                return Ok(false);
            }
        }
        if let Some(max) = c.max {
            if facts.size > max {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "byte_range",
            "unit": "bytes",
            "hint": "Inclusive byte bounds. Leave empty for any.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(size: u64) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            size,
            ..Default::default()
        }
    }

    #[test]
    fn bounds() {
        let c = FileSize;
        let v = json!({ "min": 100, "max": 200 });
        assert!(c.match_facts(&v, &facts(100)).unwrap());
        assert!(c.match_facts(&v, &facts(200)).unwrap());
        assert!(!c.match_facts(&v, &facts(99)).unwrap());
        assert!(!c.match_facts(&v, &facts(201)).unwrap());
    }

    #[test]
    fn open_bound() {
        let c = FileSize;
        assert!(c.match_facts(&json!({ "min": 100 }), &facts(10_000)).unwrap());
        assert!(c.match_facts(&json!({ "max": 100 }), &facts(50)).unwrap());
        assert!(c.match_facts(&json!({}), &facts(50)).unwrap());
    }
}
