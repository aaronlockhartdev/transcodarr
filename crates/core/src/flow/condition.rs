use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A step's condition: an **AND** of per-field constraints (DESIGN §2, §5).
///
/// Each entry's key is a condition field's `key` (see
/// [`crate::registry::ConditionField`]) and its value is the field's own
/// JSON constraint — parsed and matched by that field's registry entry.
/// Fields absent from the map are **"any"**.
///
/// OR is expressed by duplicating a step with the same operation
/// (DESIGN §5; OR-groups are a parked extension).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Per-field constraints, keyed by condition-field key.
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

impl Condition {
    /// A constraint is "any" when absent or the field's empty form (`{}`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// True when this condition matches every file (empty condition).
    #[must_use]
    pub fn is_catch_all(&self) -> bool {
        self.fields.is_empty()
    }
}
