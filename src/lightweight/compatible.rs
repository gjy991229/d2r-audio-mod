//! Resource reconstruction and configuration validation helpers.
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct Origin {
    pub path: String,
    pub method: String,
    pub game_source: Option<String>,
    pub equality: String,
}
pub struct Generated {
    pub bytes: Vec<u8>,
    pub method: &'static str,
    pub game_source: Option<String>,
    pub semantic_json: bool,
    pub native_alternative: bool,
    pub omit_file: bool,
}
fn parsed(b: &[u8]) -> Option<Value> {
    super::parse(b).ok()
}
fn read(storage: &casc_core::Storage, path: &str) -> Result<Option<Vec<u8>>, String> {
    match storage.read(&format!("data:{}", path.replace('/', "\\"))) {
        Ok(b) => Ok(Some(b)),
        Err(casc_core::CascError::Backend {
            op: "CascOpenFile",
            code: 2,
        })
        | Err(casc_core::CascError::NotFound(_)) => Ok(None),
        Err(e) => Err(format!("原版资源读取失败 {path}: {e}")),
    }
}
/// Keep matching native values; apply reference deletions, additions and edits.
fn reconcile(native: &mut Value, reference: &Value) {
    if native == reference {
        return;
    }
    match reference {
        Value::Object(map) => {
            if !native.is_object() {
                *native = serde_json::json!({});
            }
            let out = native.as_object_mut().unwrap();
            out.retain(|k, _| map.contains_key(k));
            for (key, value) in map {
                reconcile(out.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        Value::Array(items) => {
            if !native.is_array() {
                *native = serde_json::json!([]);
            }
            let out = native.as_array_mut().unwrap();
            out.resize(items.len(), Value::Null);
            for (a, b) in out.iter_mut().zip(items) {
                reconcile(a, b);
            }
        }
        _ => *native = reference.clone(),
    }
}
fn u32at(b: &[u8], p: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?) as usize)
}
struct Sprite {
    w: usize,
    h: usize,
    n: usize,
    fw: usize,
    len: usize,
}
impl Sprite {
    fn read(b: &[u8]) -> Option<Self> {
        if b.len() < 40
            || !(b.starts_with(b"SpA1") || b.starts_with(b"SPa1"))
            || b[4..6] != 31u16.to_le_bytes()
        {
            return None;
        }
        let (w, h, n) = (u32at(b, 8)?, u32at(b, 12)?, u32at(b, 20)?);
        let fw = u16::from_le_bytes(b[6..8].try_into().ok()?) as usize;
        let len = w.checked_mul(h)?.checked_mul(4)?;
        if w == 0
            || h == 0
            || n == 0
            || fw == 0
            || len > 512 * 1024 * 1024
            || 40 + len > b.len()
            || fw.checked_mul(n)? > w
        {
            return None;
        }
        Some(Self { w, h, n, fw, len })
    }
    fn cell(&self, i: usize) -> Option<(usize, usize)> {
        if i >= self.n {
            return None;
        }
        let extra = self.w - self.n * self.fw;
        if extra % self.n == 0 && extra / self.n <= 2 {
            let stride = self.fw + extra / self.n;
            return Some((i * stride, stride));
        }
        if self.n > 1 && extra % (self.n - 1) == 0 && extra / (self.n - 1) <= 2 {
            let gap = extra / (self.n - 1);
            return Some((
                i * (self.fw + gap),
                self.fw + if i + 1 == self.n { 0 } else { gap },
            ));
        }
        None
    }
}
/// Reference geometry is a target, never a source of pixels or erase masks.
/// Keep native logical frame dimensions; only remove animation frames/padding.
fn sprite_candidate(native: &[u8], reference: &[u8]) -> Option<Vec<u8>> {
    let rg = Sprite::read(reference)?;
    sprite_geometry(native, [rg.w, rg.h, rg.n, rg.fw]).ok()
}
/// Reconstruct from numeric geometry only; no reference pixels are needed.
pub(super) fn sprite_geometry(native: &[u8], spec: [usize; 4]) -> Result<Vec<u8>, String> {
    sprite_geometry_inner(native, spec).ok_or_else(|| "原版 sprite 与内置帧几何不匹配".into())
}
fn sprite_geometry_inner(native: &[u8], [w, h, n, fw]: [usize; 4]) -> Option<Vec<u8>> {
    let ng = Sprite::read(native)?;
    let len = w.checked_mul(h)?.checked_mul(4)?;
    if w == 0 || h == 0 || n == 0 || fw == 0 || len > 512 * 1024 * 1024 || fw.checked_mul(n)? > w {
        return None;
    }
    let rg = Sprite { w, h, n, fw, len };
    if ng.h != rg.h || !(ng.n == rg.n || rg.n == 1) {
        return None;
    }
    {
        let mut pixels = Vec::with_capacity(rg.len);
        for y in 0..rg.h {
            for i in 0..rg.n {
                let (sx, sw) = ng.cell(i)?;
                let (_, dw) = rg.cell(i)?;
                if dw > sw || dw < ng.fw {
                    return None;
                }
                let from = 40 + (y * ng.w + sx) * 4;
                pixels.extend_from_slice(&native[from..from + dw * 4]);
            }
        }
        if pixels.len() != rg.len {
            return None;
        }
        let mut out = native[..40].to_vec();
        for (offset, value) in [(8, rg.w), (20, rg.n), (32, rg.len), (36, 4)] {
            out[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        out.extend_from_slice(&pixels);
        return Some(out);
    }
}
fn missing_target_can_be_omitted(path: &str) -> bool {
    path.ends_with(".texture") || path == "data/hd/global/textures/smaa/smaa_texture_EXPERIMENT.dds"
}
fn is_pause_layout(path: &str) -> bool {
    matches!(
        path,
        "data/global/ui/layouts/pauselayout.json"
            | "data/global/ui/layouts/pauselayoutgarden.json"
            | "data/global/ui/layouts/pauselayouthd.json"
            | "data/global/ui/layouts/pauselayoutgardenhd.json"
    )
}
/// Remove automatic exit timers only; keep manual Save and Exit buttons intact.
fn remove_auto_exit(value: &mut Value) -> usize {
    let mut removed = 0;
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                removed += remove_auto_exit(child);
            }
        }
        Value::Array(items) => {
            let before = items.len();
            items.retain(|v| {
                !(v["type"] == "TimerWidget"
                    && v["fields"]["message"] == "PausePanelMessage:ExitGame")
            });
            removed += before - items.len();
            for child in items {
                removed += remove_auto_exit(child);
            }
        }
        _ => {}
    }
    removed
}
pub fn generate(
    storage: &casc_core::Storage,
    path: &str,
    reference: &[u8],
) -> Result<Generated, String> {
    let reuse = |method| Generated {
        bytes: reference.to_vec(),
        method,
        game_source: None,
        semantic_json: false,
        native_alternative: false,
        omit_file: false,
    };
    if reference.is_empty() {
        return Ok(reuse("empty_override"));
    }
    if !path.starts_with("data/") {
        if path.ends_with(".txt") || path.ends_with(".json") {
            return Ok(reuse("reference_support_configuration"));
        }
        return Err(format!("参考包外附二进制不能作为生成素材：{path}"));
    }
    let native = read(storage, path)?;
    if native.as_deref() == Some(reference) {
        return Ok(Generated {
            bytes: native.unwrap(),
            method: "game_identical",
            game_source: Some(path.into()),
            semantic_json: false,
            native_alternative: false,
            omit_file: false,
        });
    }
    let ext = path.rsplit('.').next().unwrap_or("");
    if matches!(ext, "json" | "frontend") {
        if let Some(mut wanted) = parsed(reference) {
            let exit_removed = if is_pause_layout(path) {
                remove_auto_exit(&mut wanted)
            } else {
                0
            };
            let (source, base) = if native.is_none() && path == "data/hd/env/biome/default.json" {
                (
                    "data/hd/env/biome/act1_outdoors.json",
                    read(storage, "data/hd/env/biome/act1_outdoors.json")?,
                )
            } else {
                (path, native.clone())
            };
            if let Some(mut value) = base.as_deref().and_then(parsed) {
                reconcile(&mut value, &wanted);
                if value != wanted {
                    return Err(format!("JSON 兼容规则验证失败 {path}"));
                }
                let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
                return Ok(Generated {
                    bytes,
                    method: if exit_removed > 0 {
                        "game_json_reference_rules_no_auto_exit"
                    } else {
                        "game_json_reference_rules"
                    },
                    game_source: Some(source.into()),
                    semantic_json: true,
                    native_alternative: false,
                    omit_file: false,
                });
            }
        }
        return Ok(reuse("reference_custom_or_unparsed_definition"));
    }
    if let Some(native) = &native {
        if ext == "texture" && reference.len() >= 36 {
            let w = u32at(reference, 8).unwrap_or(0);
            let h = u32at(reference, 12).unwrap_or(0);
            if w > 0 && h > 0 && w.max(h) <= 16384 {
                if let Ok(out) = super::assets::texture(native, w.max(h).next_power_of_two()) {
                    return Ok(Generated {
                        bytes: out,
                        method: "game_texture_mip",
                        game_source: Some(path.into()),
                        semantic_json: false,
                        native_alternative: true,
                        omit_file: false,
                    });
                }
            }
        }
        if ext == "sprite" {
            if let Some(out) = sprite_candidate(native, reference) {
                return Ok(Generated {
                    bytes: out,
                    method: "game_sprite_geometry_only",
                    game_source: Some(path.into()),
                    semantic_json: false,
                    native_alternative: true,
                    omit_file: false,
                });
            }
            if !path.ends_with(".lowend.sprite") {
                let candidate = format!("{}.lowend.sprite", path.strip_suffix(".sprite").unwrap());
                if let Some(low) = read(storage, &candidate)? {
                    if let Some(out) = sprite_candidate(&low, reference) {
                        return Ok(Generated {
                            bytes: out,
                            method: "game_lowend_geometry_only",
                            game_source: Some(candidate),
                            semantic_json: false,
                            native_alternative: true,
                            omit_file: false,
                        });
                    }
                }
            }
        }
    }
    // Text configuration remains an attributed reference rule. Never use this
    // branch for sprites, textures, particle binaries, DDS, DC6 or unknown data.
    if ext == "txt" {
        return Ok(reuse("reference_text_configuration"));
    }
    if let Some(bytes) = native {
        return Ok(Generated {
            bytes,
            method: "game_original_unsimplified",
            game_source: Some(path.into()),
            semantic_json: false,
            native_alternative: true,
            omit_file: false,
        });
    }
    // Missing original texture targets cannot be simplified. Keep their absence,
    // rather than inventing pixels or changing references. Native definitions can
    // themselves contain stale preload entries. The manifest records each omission.
    if missing_target_can_be_omitted(path) {
        return Ok(Generated {
            bytes: Vec::new(),
            method: "omitted_missing_native_asset",
            game_source: None,
            semantic_json: false,
            native_alternative: true,
            omit_file: true,
        });
    }
    Err(format!("无法从游戏生成资源，禁止复制参考二进制：{path}"))
}
pub fn equal_json(path: &str, a: &[u8], b: &[u8]) -> bool {
    match (parsed(a), parsed(b)) {
        (Some(a), Some(mut b)) => {
            if is_pause_layout(path) {
                remove_auto_exit(&mut b);
            }
            a == b
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pause_timer_removal_preserves_manual_exit_and_other_timers() {
        let reference = serde_json::json!({"children":[
            {"type":"ButtonWidget","fields":{"onClickMessage":"PausePanelMessage:ExitGame"}},
            {"type":"Widget","children":[
                {"type":"TimerWidget","fields":{"message":"PausePanelMessage:ExitGame","time":0}},
                {"type":"TimerWidget","fields":{"message":"OtherMessage","time":10}}
            ]}
        ]});
        let mut actual = reference.clone();
        assert_eq!(remove_auto_exit(&mut actual), 1);
        assert_eq!(actual["children"][0], reference["children"][0]);
        assert_eq!(
            actual["children"][1]["children"].as_array().unwrap().len(),
            1
        );
        let a = serde_json::to_vec(&actual).unwrap();
        let b = serde_json::to_vec(&reference).unwrap();
        assert!(equal_json(
            "data/global/ui/layouts/pauselayouthd.json",
            &a,
            &b
        ));
        assert!(!equal_json(
            "data/global/ui/layouts/pauselayouthd.json",
            &b,
            &b
        ));
        assert!(!equal_json("data/unrelated.json", &a, &b));
    }
    #[test]
    fn required_skeleton_and_reference_controls_are_not_simplified_away() {
        let mut native = serde_json::json!({"name":"native","entities":[{"id":1,"components":[{"type":"UnitRootComponent","state_machine_filename":"motion"},{"type":"SkeletonDefinitionComponent","filename":"real.skeleton"},{"type":"ModelDefinitionComponent"}]}],"dependencies":{"models":["body"]}});
        let reference = serde_json::json!({"name":"null","entities":[{"id":999,"components":[{"type":"UnitRootComponent","state_machine_filename":""},{"type":"SkeletonDefinitionComponent","filename":"data/hd/objects/dummy/null/skeleton/null.skeleton"}]}]});
        reconcile(&mut native, &reference);
        assert_eq!(native, reference);
        assert_eq!(
            native["entities"][0]["components"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn reference_pixels_and_masks_never_supply_output_pixels() {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 2;
        for (p, n) in [(8, 2u32), (12, 1), (20, 1)] {
            b[p..p + 4].copy_from_slice(&n.to_le_bytes());
        }
        b.extend([255, 0, 0, 255, 0, 255, 0, 255]);
        let mut reference = b.clone();
        reference[40..44].fill(0);
        let out = sprite_candidate(&b, &reference).unwrap();
        assert_eq!(&out[40..], &b[40..]);
        reference[44..48].copy_from_slice(&[0, 0, 255, 255]);
        assert_eq!(sprite_candidate(&b, &reference).unwrap(), out);
    }
    #[test]
    fn sprite_frame_reduction_preserves_logical_dimensions() {
        let mut b = vec![0; 40];
        b[..4].copy_from_slice(b"SpA1");
        b[4] = 31;
        b[6] = 2;
        for (p, n) in [(8, 8u32), (12, 2), (20, 2)] {
            b[p..p + 4].copy_from_slice(&n.to_le_bytes());
        }
        b.extend(0..64u8);
        let mut reference = b.clone();
        reference[8..12].copy_from_slice(&4u32.to_le_bytes());
        reference[20..24].copy_from_slice(&1u32.to_le_bytes());
        let out = sprite_candidate(&b, &reference).unwrap();
        assert_eq!(out[6], 2);
        assert_eq!(out.len(), 40 + 4 * 2 * 4);
        assert_eq!(&out[40..56], &b[40..56]);
        assert_eq!(&out[56..72], &b[72..88]);
        reference[12..16].copy_from_slice(&1u32.to_le_bytes());
        assert!(sprite_candidate(&b, &reference).is_none());
    }
}
