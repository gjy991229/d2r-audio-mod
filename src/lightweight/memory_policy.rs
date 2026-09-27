//! Independent load-chain pruning. Never edit state-machine contents or synthesize
//! animation/skeleton assets. Preserve motion required by surviving native roots.
use super::native_policy::resource_path;
use serde_json::{json, Value};
use std::collections::HashSet;

pub const SHARED_BIOME: &str = "data/hd/env/biome/default.json";
pub const BIOME_BASE: &str = "data/hd/env/biome/act1_outdoors.json";
pub const DEFAULT_VIS: &str = "data/hd/env/vis/1_default_day.json";
pub const TERRAIN_CHANNELS: [&str; 4] = ["Albedo", "Normal", "ORM", "Noise"];
pub fn tiny_terrain_path(channel: &str) -> String {
    format!(
        "data/hd/env/texture/d2rlight/terrain_{}.texture",
        channel.to_ascii_lowercase()
    )
}

fn refs(v: &Value, out: &mut HashSet<String>) {
    match v {
        Value::String(s) => {
            let p = resource_path(s);
            if p.starts_with("data/") {
                out.insert(p);
            }
        }
        Value::Array(a) => {
            for v in a {
                refs(v, out);
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                if k != "dependencies" {
                    refs(v, out);
                }
            }
        }
        _ => {}
    }
}

/// A native biome shell with original layer shapes but shared tiny material maps.
/// Keep native lighting values in the original default visual document.
pub fn biome(value: &mut Value) -> usize {
    let before = value.clone();
    if let Some(map) = value.as_object_mut() {
        map.retain(|k, _| {
            matches!(
                k.as_str(),
                "type"
                    | "name"
                    | "visualDataFilenames"
                    | "iblSettings"
                    | "terrainDataUltra"
                    | "terrainDataHigh"
                    | "terrainDataMed"
                    | "terrainDataLow"
            )
        });
        for k in [
            "terrainDataUltra",
            "terrainDataHigh",
            "terrainDataMed",
            "terrainDataLow",
        ] {
            if let Some(tier) = map.get_mut(k).and_then(Value::as_object_mut) {
                tier.retain(|k, _| k == "terrainLayers");
                if let Some(layers) = tier.get_mut("terrainLayers").and_then(Value::as_array_mut) {
                    for layer in layers {
                        if let Some(fields) = layer.as_object_mut() {
                            for channel in TERRAIN_CHANNELS {
                                if fields.contains_key(channel) {
                                    fields
                                        .insert(channel.into(), json!(tiny_terrain_path(channel)));
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(items) = map
            .get_mut("visualDataFilenames")
            .and_then(Value::as_array_mut)
        {
            for item in items {
                if let Some(field) = item.get_mut("visualDataFilename") {
                    *field = json!(DEFAULT_VIS);
                }
            }
        }
        let mut used = HashSet::new();
        refs(&Value::Object(map.clone()), &mut used);
        let mut json_paths = Vec::new();
        let mut others = Vec::new();
        let mut textures = Vec::new();
        let mut used = used.into_iter().collect::<Vec<_>>();
        used.sort();
        for p in used {
            if p.ends_with(".json") {
                json_paths.push(json!({"path":p}));
            } else if p.ends_with(".texture") {
                textures.push(json!({"path":p}));
            } else {
                others.push(json!({"path":p}));
            }
        }
        map.insert(
            "dependencies".into(),
            json!({"json":json_paths,"other":others,"textures":textures}),
        );
    }
    usize::from(*value != before)
}

pub fn apply(value: &mut Value, shared_biome: bool) -> usize {
    let before = value.clone();
    match value.get("type").and_then(Value::as_str) {
        Some("Biome") if shared_biome => {
            biome(value);
        }
        Some("Preset") => {
            let map = value.as_object_mut().unwrap();
            map.remove("terrain");
            if map.contains_key("perTileBiomeOverrides") {
                map.insert("perTileBiomeOverrides".into(), json!([]));
            }
            if map.contains_key("specialTiles") {
                map.insert("specialTiles".into(), json!({}));
            }
            if shared_biome {
                map.insert("biomeFilename".into(), json!(SHARED_BIOME));
            }
            let mut used = HashSet::new();
            refs(value, &mut used);
            if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
                for list in deps.values_mut() {
                    if let Some(a) = list.as_array_mut() {
                        a.retain(|v| {
                            v.as_str()
                                .or_else(|| v.get("path").and_then(Value::as_str))
                                .is_none_or(|p| used.contains(&resource_path(p)))
                        });
                    }
                }
                if shared_biome {
                    deps.insert("json".into(), json!([{"path":SHARED_BIOME}]));
                }
            }
        }
        Some("UnitDefinition" | "OverlayDefinition") => {
            let mut used = HashSet::new();
            refs(value, &mut used);
            let motion = has_motion(value);
            if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
                // JSON and texture dependencies may be implicit in a surviving
                // model/state machine: do not assume they are dead from this file.
                for kind in ["models", "skeletons", "animations"] {
                    if kind == "animations" && motion {
                        continue;
                    }
                    if let Some(a) = deps.get_mut(kind).and_then(Value::as_array_mut) {
                        a.retain(|v| {
                            v.as_str()
                                .or_else(|| v.get("path").and_then(Value::as_str))
                                .is_none_or(|p| used.contains(&resource_path(p)))
                        });
                    }
                }
            }
        }
        _ => {}
    }
    usize::from(*value != before)
}

fn has_motion(v: &Value) -> bool {
    match v {
        Value::String(s) => s.ends_with(".animation") || s.ends_with(".animations"),
        Value::Array(a) => a.iter().any(has_motion),
        Value::Object(m) => m.iter().any(|(k, v)| {
            if k == "dependencies" {
                return false;
            }
            if k == "state_machine_filename" && v.as_str().is_some_and(|s| !s.is_empty()) {
                return true;
            }
            if k == "animations" && v.as_array().is_some_and(|a| !a.is_empty()) {
                return true;
            }
            if k == "type" && v.as_str() == Some("AnimationStateMachine") {
                return true;
            }
            has_motion(v)
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn biome_and_preset_cut_terrain_chains_without_author_parameters() {
        let mut b = json!({"type":"Biome","name":"native","tileMaskFilename":"data/huge.texture","terrainDataHigh":{"terrainLayers":[{"Albedo":"data/alb.texture"}],"foliage":"data/grass.json"},"visualDataFilenames":[{"visualDataFilename":"data/old.json"}],"iblSettings":{"cubemapFilename":"data/native.ibls"}});
        biome(&mut b);
        assert_eq!(b["name"], "native");
        assert!(b.get("tileMaskFilename").is_none());
        assert_eq!(
            b["terrainDataHigh"]["terrainLayers"][0]["Albedo"],
            tiny_terrain_path("Albedo")
        );
        assert!(b["terrainDataHigh"].get("foliage").is_none());
        let mut p = json!({"type":"Preset","terrain":{"x":"data/huge.texture"},"entities":[],"biomeFilename":"data/old.json","dependencies":{"json":[{"path":"data/old.json"}],"textures":[{"path":"data/huge.texture"}]}});
        apply(&mut p, true);
        assert!(p.get("terrain").is_none());
        assert_eq!(p["biomeFilename"], SHARED_BIOME);
        assert_eq!(p["dependencies"]["json"][0]["path"], SHARED_BIOME);
        assert_eq!(p["dependencies"]["textures"], json!([]));
    }
    #[test]
    fn orphan_motion_preloads_removed_but_live_state_machine_preserved() {
        let mut v = json!({"type":"UnitDefinition","entities":[{"components":[{"type":"UnitRootComponent","state_machine_filename":"data/state.json"},{"type":"SkeletonDefinitionComponent","filename":"data/live.skeleton"}]}],"dependencies":{"skeletons":[{"path":"data/live.skeleton"},{"path":"data/orphan.skeleton"}],"animations":[{"path":"data/motion.animation"}]}});
        apply(&mut v, false);
        assert_eq!(v["dependencies"]["skeletons"].as_array().unwrap().len(), 1);
        assert_eq!(v["dependencies"]["animations"].as_array().unwrap().len(), 1);
        assert_eq!(
            v["entities"][0]["components"][0]["state_machine_filename"],
            "data/state.json"
        );
        v["entities"][0]["components"][0]["state_machine_filename"] = json!("");
        apply(&mut v, false);
        assert_eq!(v["dependencies"]["animations"], json!([]));
    }
}
