//! Independent, bounded operations on installed game assets. No reference values,
//! IDs, image masks, frame matches or replacement documents are consumed here.
use serde_json::Value;
use std::collections::HashSet;

pub fn resource_path(s: &str) -> String {
    let p = s
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if p.starts_with("hd/") || p.starts_with("global/") {
        format!("data/{p}")
    } else {
        p
    }
}
fn blocked(s: &str, disabled: &HashSet<String>) -> bool {
    disabled.contains(&resource_path(s))
}

pub fn is_vfx_texture(path: &str) -> bool {
    let p = resource_path(path);
    p.ends_with(".texture")
        && (p.starts_with("data/hd/vfx/")
            || p.rsplit('/').next().is_some_and(|n| n.starts_with("fx_")))
}

/// Particle suppression follows actual references in already-scoped native JSON.
/// It does not import a third-party particle list or scan unrelated game assets.
pub fn particle_references(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::String(s) => {
            let p = resource_path(s);
            if p.ends_with(".particles") && super::recipe::safe_path(&p) {
                out.insert(p);
            }
        }
        Value::Array(a) => {
            for v in a {
                particle_references(v, out);
            }
        }
        Value::Object(m) => {
            for v in m.values() {
                particle_references(v, out);
            }
        }
        _ => {}
    }
}

/// Clear only references to resources explicitly disabled in this profile.
/// Preserve native names, IDs, numbers, transforms and all unrelated components.
pub fn json(value: &mut Value, disabled: &HashSet<String>) -> usize {
    // Scoped HD scene presets describe decorative scene entities. Use a single
    // schema-based rule instead of reproducing an author's per-file entity list.
    let mut preset_changes = 0;
    if value.get("type").and_then(Value::as_str) == Some("Preset") {
        if let Some(entities) = value.get_mut("entities").and_then(Value::as_array_mut) {
            preset_changes += entities.len();
            entities.clear();
        }
        fn refs(value: &Value, out: &mut HashSet<String>) {
            match value {
                Value::String(s) => {
                    out.insert(resource_path(s));
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
        let mut retained = HashSet::new();
        refs(value, &mut retained);
        if let Some(deps) = value.get_mut("dependencies").and_then(Value::as_object_mut) {
            for list in deps.values_mut() {
                if let Some(items) = list.as_array_mut() {
                    items.retain(|v| {
                        let p = v.as_str().or_else(|| v.get("path").and_then(Value::as_str));
                        let remove = p.is_some_and(|p| !retained.contains(&resource_path(p)));
                        if remove {
                            preset_changes += 1;
                        }
                        !remove
                    });
                }
            }
        }
    }
    fn visit(v: &mut Value, disabled: &HashSet<String>, count: &mut usize) {
        match v {
            Value::String(s) if blocked(s, disabled) => {
                s.clear();
                *count += 1;
            }
            Value::Array(items) => {
                for item in items {
                    visit(item, disabled, count);
                }
            }
            Value::Object(map) => {
                if let Some(Value::Object(deps)) = map.get_mut("dependencies") {
                    for list in deps.values_mut() {
                        if let Some(items) = list.as_array_mut() {
                            items.retain(|item| {
                                let p = item
                                    .as_str()
                                    .or_else(|| item.get("path").and_then(Value::as_str));
                                let remove = p.is_some_and(|p| blocked(p, disabled));
                                if remove {
                                    *count += 1;
                                }
                                !remove
                            });
                        }
                    }
                }
                if let Some(Value::Array(components)) = map.get_mut("components") {
                    components.retain(|c| {
                        let visual = matches!(
                            c.get("type").and_then(Value::as_str),
                            Some(
                                "ModelDefinitionComponent"
                                    | "VfxDefinitionComponent"
                                    | "SkeletonDefinitionComponent"
                                    | "DecalDefinitionComponent"
                                    | "TerrainDecalDefinitionComponent"
                            )
                        );
                        let disabled_file = c
                            .get("filename")
                            .and_then(Value::as_str)
                            .is_some_and(|p| blocked(p, disabled));
                        let remove = visual && disabled_file;
                        if remove {
                            *count += 1;
                        }
                        !remove
                    });
                }
                for child in map.values_mut() {
                    visit(child, disabled, count);
                }
            }
            _ => {}
        }
    }
    let mut count = preset_changes;
    visit(value, disabled, &mut count);
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_paths_are_bounded_and_do_not_include_audio_or_invalid_paths() {
        let value = serde_json::json!({"vfx":"DATA\\HD\\VFX\\hit.particles","audio":"data/hd/sound.flac","bad":"data/../escape.particles","other":["data/hd/vfx/hit.particles"]});
        let mut out = HashSet::new();
        particle_references(&value, &mut out);
        assert_eq!(out, HashSet::from(["data/hd/vfx/hit.particles".into()]));
        assert!(is_vfx_texture("DATA/HD/VFX/textures/a.texture"));
        assert!(!is_vfx_texture("data/hd/character/a.texture"));
    }
    #[test]
    fn clears_only_blocked_resources_without_importing_custom_values() {
        let mut v = serde_json::json!({"id":42,"power":35,"name":"native","dependencies":{"models":[{"path":"data/hd/a.model"},{"path":"data/hd/keep.model"}]},"entities":[{"id":7,"components":[{"type":"ModelDefinitionComponent","filename":"DATA\\HD\\A.MODEL"},{"type":"UnitRootComponent","state_machine_filename":"data/hd/a.json"},{"type":"TransformDefinitionComponent","position":{"x":10}}]}]});
        let disabled = HashSet::from(["data/hd/a.model".into(), "data/hd/a.json".into()]);
        assert_eq!(json(&mut v, &disabled), 3);
        assert_eq!(v["id"], 42);
        assert_eq!(v["power"], 35);
        assert_eq!(v["name"], "native");
        assert_eq!(v["dependencies"]["models"].as_array().unwrap().len(), 1);
        assert_eq!(
            v["entities"][0]["components"][0]["state_machine_filename"],
            ""
        );
        assert_eq!(v["entities"][0]["components"][1]["position"]["x"], 10);
        assert_eq!(json(&mut v, &disabled), 0);
    }
    #[test]
    fn preset_rule_culls_scene_entities_but_keeps_native_terrain_and_biome() {
        let mut v = serde_json::json!({"type":"Preset","name":"native","biomeFilename":"data/hd/biome.json","terrain":{"filename":"data/hd/floor.model"},"entities":[{"id":123}],"dependencies":{"json":[{"path":"data/hd/biome.json"}],"models":[{"path":"data/hd/floor.model"},{"path":"data/hd/decor.model"}]}});
        assert_eq!(json(&mut v, &HashSet::new()), 2);
        assert_eq!(v["name"], "native");
        assert!(v["entities"].as_array().unwrap().is_empty());
        assert_eq!(v["terrain"]["filename"], "data/hd/floor.model");
        assert_eq!(v["dependencies"]["models"].as_array().unwrap().len(), 1);
        assert_eq!(v["dependencies"]["json"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn empty_scope_does_not_change_native_definition() {
        let mut v = serde_json::json!({"filename":"data/hd/a.model","power":99,"dependencies":{"json":["data/hd/source.json"]}});
        let old = v.clone();
        assert_eq!(json(&mut v, &HashSet::new()), 0);
        assert_eq!(v, old);
    }
}
