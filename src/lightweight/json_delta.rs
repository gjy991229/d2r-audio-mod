//! Native JSON plus explicit reference edits. Arrays are positional only because
//! the complete native input is fingerprinted; changed game data must be reviewed.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum Delta {
    Keep,
    Scalar(Value),
    Object {
        remove: Vec<String>,
        edits: BTreeMap<String, Delta>,
    },
    Array(Vec<Delta>),
}
impl Delta {
    fn between(native: &Value, reference: &Value) -> Self {
        if native == reference {
            return Self::Keep;
        }
        match reference {
            Value::Object(map) => Self::Object {
                remove: native
                    .as_object()
                    .map(|m| {
                        m.keys()
                            .filter(|k| !map.contains_key(*k))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default(),
                edits: map
                    .iter()
                    .filter_map(|(key, value)| {
                        let old = native.get(key).unwrap_or(&Value::Null);
                        (old != value || native.get(key).is_none())
                            .then(|| (key.clone(), Self::between(old, value)))
                    })
                    .collect(),
            },
            Value::Array(items) => Self::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Self::between(native.get(i).unwrap_or(&Value::Null), v))
                    .collect(),
            ),
            _ => Self::Scalar(reference.clone()),
        }
    }
    fn apply(&self, native: &mut Value) -> Result<(), String> {
        match self {
            Self::Keep => {}
            Self::Scalar(value) => {
                if value.is_object() || value.is_array() {
                    return Err("JSON scalar edit cannot contain a document".into());
                }
                *native = value.clone();
            }
            Self::Object { remove, edits } => {
                if !native.is_object() {
                    *native = serde_json::json!({});
                }
                let map = native.as_object_mut().unwrap();
                for key in remove {
                    map.remove(key);
                }
                for (key, edit) in edits {
                    edit.apply(map.entry(key.clone()).or_insert(Value::Null))?;
                }
            }
            Self::Array(edits) => {
                if !native.is_array() {
                    *native = serde_json::json!([]);
                }
                let items = native.as_array_mut().unwrap();
                items.resize(edits.len(), Value::Null);
                for (value, edit) in items.iter_mut().zip(edits) {
                    edit.apply(value)?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub source: String,
    pub native_fingerprint: String,
    pub delta: Delta,
}
impl Plan {
    pub fn derive(source: &str, native: &[u8], reference: &Value) -> Result<Self, String> {
        let value = super::recipe::parse(native)?;
        let plan = Self {
            source: source.into(),
            native_fingerprint: super::reference::fingerprint(native),
            delta: Delta::between(&value, reference),
        };
        if plan.apply(native)? != *reference {
            return Err("JSON delta did not reproduce reference".into());
        }
        Ok(plan)
    }
    pub fn apply(&self, native: &[u8]) -> Result<Value, String> {
        if super::reference::fingerprint(native) != self.native_fingerprint {
            return Err("原版 JSON 已变化，需要重新核对引用和参数规则".into());
        }
        let mut value = super::recipe::parse(native)?;
        self.delta.apply(&mut value)?;
        Ok(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_parameters_and_dependency_lists_match_reference() {
        let native=br#"{"biomeFilename":"old.json","dependencies":{"json":[]},"power":35,"flag":false,"entities":[{"name":"root","filename":"amazon.skeleton"}],"remove":1}"#;
        let reference = serde_json::json!({"biomeFilename":"data/hd/env/biome/default.json","dependencies":{"json":["data/hd/env/biome/default.json"]},"power":25,"flag":true,"entities":[{"name":"root","filename":"data/hd/objects/dummy/null/skeleton/null.skeleton"}],"terrain":{"Albedo":"fake.texture"},"empty":[],"null":null});
        let plan = Plan::derive("data/test.json", native, &reference).unwrap();
        let encoded = serde_json::to_vec(&plan).unwrap();
        let plan: Plan = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(plan.apply(native).unwrap(), reference);
        assert!(plan.apply(br#"{}"#).is_err());
    }
    #[test]
    fn array_growth_removal_and_type_changes_are_exact() {
        for (a, b) in [
            (
                serde_json::json!([1, 2]),
                serde_json::json!([null,{"x":4},3]),
            ),
            (serde_json::json!({"a":1}), serde_json::json!([])),
            (serde_json::json!([1, 2]), serde_json::json!([1])),
        ] {
            let mut actual = a.clone();
            Delta::between(&a, &b).apply(&mut actual).unwrap();
            assert_eq!(actual, b);
        }
    }
}
