//! Explicit profile/category optimizations, separate from pixel-layout UI panels.
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub const HEALTH_IDLE: &str =
    "data/hd/global/ui/panel/hud_02/healthmanaanimation/healthidle/4k/globe_health_man_idle.sprite";

pub fn is_map(path: &str) -> bool {
    path.starts_with("data/hd/global/ui/automap/") && path.ends_with(".sprite")
}
pub fn special_sprite(profile: &str, path: &str) -> bool {
    (profile == "min" && is_map(path)) || (profile == "filler" && path == HEALTH_IDLE)
}

/// Preserve native identity/root transform, but remove all HD render/motion chains.
/// This is deliberately restricted to the existing min UnitDefinition targets.
pub fn no_model(value: &mut Value) -> Result<(), String> {
    let entities = value
        .get("entities")
        .and_then(Value::as_array)
        .ok_or("无模型实体缺少原版 entities")?;
    let root = entities
        .iter()
        .find(|e| {
            e.get("components")
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|c| c["type"] == "UnitRootComponent"))
        })
        .ok_or("没有原版 UnitRootComponent，未猜测实体结构")?;
    let mut root = root.clone();
    let comps = root["components"].as_array_mut().unwrap();
    comps.retain(|c| {
        matches!(
            c.get("type").and_then(Value::as_str),
            Some("UnitRootComponent" | "TransformDefinitionComponent")
        )
    });
    for c in comps.iter_mut() {
        if c["type"] == "UnitRootComponent" {
            c["state_machine_filename"] = json!("");
            c["animations"] = json!([]);
            if c.get("onCreateEventName").is_some() {
                c["onCreateEventName"] = json!("");
            }
        }
    }
    value["entities"] = json!([root]);
    value["dependencies"] = json!({});
    Ok(())
}

/// Reuse native light equipment models for medium/heavy variants of the same part.
/// Keep part selectors, transforms, visibility and motion unchanged.
pub fn light_variants(value: &mut Value) -> usize {
    let Some(entities) = value.get_mut("entities").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut light = BTreeMap::new();
    for e in entities.iter() {
        if let Some(a) = e.get("components").and_then(Value::as_array) {
            let part = a
                .iter()
                .find(|c| c["type"] == "UnitPartComponent" && c["variant"] == "lit")
                .and_then(|c| c["part"].as_str());
            let model = a
                .iter()
                .find(|c| c["type"] == "ModelDefinitionComponent")
                .and_then(|c| c["filename"].as_str());
            if let Some((p, m)) = part.zip(model) {
                if !m.is_empty() {
                    light.insert(p.to_string(), m.to_string());
                }
            }
        }
    }
    let mut count = 0;
    for e in entities.iter_mut() {
        if let Some(a) = e.get_mut("components").and_then(Value::as_array_mut) {
            let part = a
                .iter()
                .find(|c| {
                    c["type"] == "UnitPartComponent"
                        && matches!(c["variant"].as_str(), Some("med" | "hvy"))
                })
                .and_then(|c| c["part"].as_str())
                .map(str::to_string);
            if let Some(path) = part.and_then(|p| light.get(&p)) {
                for c in a {
                    if c["type"] == "ModelDefinitionComponent" && c["filename"] != path.as_str() {
                        c["filename"] = json!(path);
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn u32at(b: &[u8], p: usize) -> Result<usize, String> {
    Ok(u32::from_le_bytes(b.get(p..p + 4).ok_or("短 sprite 头")?.try_into().unwrap()) as usize)
}
fn geometry(b: &[u8]) -> Result<(usize, usize, usize, usize), String> {
    if b.len() < 40
        || !(b.starts_with(b"SpA1") || b.starts_with(b"SPa1"))
        || b[4..6] != 31u16.to_le_bytes()
    {
        return Err("不支持的原版 sprite 格式".into());
    }
    let (w, h, n, fw) = (
        u32at(b, 8)?,
        u32at(b, 12)?,
        u32at(b, 20)?,
        u16::from_le_bytes(b[6..8].try_into().unwrap()) as usize,
    );
    let len = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .ok_or("sprite 长度溢出")?;
    if w == 0 || h == 0 || n == 0 || fw == 0 || len > 512 * 1024 * 1024 || b.len() < 40 + len {
        return Err("无效 sprite 几何/像素长度".into());
    }
    Ok((w, h, n, fw))
}
pub fn lowend_map(high: &[u8], low: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h, n, _) = geometry(high)?;
    let (lw, lh, ln, _) = geometry(low)?;
    if n != ln || lw > w || lh > h {
        return Err("原版地图低清帧结构不匹配".into());
    }
    Ok(low[..40 + lw * lh * 4].to_vec())
}
pub fn first_frame(b: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h, n, fw) = geometry(b)?;
    if w % n != 0 || w / n < fw || w / n - fw > 2 {
        return Err("血球帧间距未识别，未猜测裁切".into());
    }
    let stride = w / n;
    let mut out = b[..40].to_vec();
    for (p, v) in [(8, stride), (20, 1), (32, stride * h * 4), (36, 4)] {
        out[p..p + 4].copy_from_slice(&(v as u32).to_le_bytes());
    }
    for y in 0..h {
        let start = 40 + y * w * 4;
        out.extend_from_slice(&b[start..start + stride * 4]);
    }
    Ok(out)
}
pub fn vfx_limit(b: &[u8]) -> Result<usize, String> {
    let (w, h) = (u32at(b, 8)?, u32at(b, 12)?);
    if w == 0 || h == 0 {
        return Err("无效 VFX 纹理尺寸".into());
    }
    // Keep long ramps substantially larger than square particle silhouettes.
    Ok(if w.min(h) <= 16 || w.max(h) / w.min(h) >= 8 {
        512
    } else {
        64
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn min_entity_keeps_native_identity_without_render_or_motion_preloads() {
        let mut v = json!({"type":"UnitDefinition","name":"native","entities":[{"id":9,"components":[{"type":"UnitRootComponent","state_machine_filename":"data/motion.json","animations":[1]},{"type":"SkeletonDefinitionComponent","filename":"data/full.skeleton"}]},{"components":[{"type":"ModelDefinitionComponent","filename":"data/body.model"}]}],"dependencies":{"textures":["data/body.texture"]}});
        no_model(&mut v).unwrap();
        assert_eq!(v["name"], "native");
        assert_eq!(v["entities"][0]["id"], 9);
        assert_eq!(v["entities"].as_array().unwrap().len(), 1);
        assert_eq!(
            v["entities"][0]["components"][0]["state_machine_filename"],
            ""
        );
        assert_eq!(v["dependencies"], json!({}));
    }
    #[test]
    fn single_frame_keeps_display_geometry_and_map_policy_excludes_inventory() {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 2;
        for (p, v) in [(8, 6u32), (12, 2), (20, 2)] {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        b.extend(0..48u8);
        let out = first_frame(&b).unwrap();
        assert_eq!(u32at(&out, 8).unwrap(), 3);
        assert_eq!(u32at(&out, 12).unwrap(), 2);
        assert_eq!(&out[6..8], &b[6..8]);
        assert_eq!(&out[40..52], &b[40..52]);
        assert_eq!(&out[52..], &b[64..76]);
        assert!(!special_sprite(
            "min",
            "data/hd/global/ui/panel/inventory/background.sprite"
        ));
        assert!(special_sprite(
            "min",
            "data/hd/global/ui/automap/act5town/tiles.sprite"
        ));
    }
    #[test]
    fn light_model_reuse_does_not_change_motion_or_part_selection() {
        let mut v = json!({"entities":[{"components":[{"type":"UnitPartComponent","part":"torso","variant":"lit"},{"type":"ModelDefinitionComponent","filename":"data/lit.model"}]},{"components":[{"type":"UnitPartComponent","part":"torso","variant":"hvy"},{"type":"ModelDefinitionComponent","filename":"data/hvy.model"}]}]});
        assert_eq!(light_variants(&mut v), 1);
        assert_eq!(v["entities"][1]["components"][0]["variant"], "hvy");
        assert_eq!(
            v["entities"][1]["components"][1]["filename"],
            "data/lit.model"
        );
    }
}
