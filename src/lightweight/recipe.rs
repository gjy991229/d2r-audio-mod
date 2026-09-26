use super::reference::{self, SpritePlan, TexturePlan};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
};

/// This is a structural selection rule, never a replacement game document.
/// Nonzero numbers, arbitrary strings, pixel data and new source-mod entities
/// are intentionally not serialized into recipes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum Shape {
    Keep,
    EmptyString,
    Zero,
    False,
    Object(BTreeMap<String, Shape>),
    Array(Vec<Selection>),
    Alias(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identity: BTreeMap<String, Value>,
    pub index: usize,
    pub shape: Shape,
}

impl Shape {
    pub fn from_reference(value: &Value) -> Self {
        match value {
            Value::String(s) if s.is_empty() => Self::EmptyString,
            Value::Bool(false) => Self::False,
            Value::Number(n) if n.as_f64() == Some(0.0) => Self::Zero,
            Value::Object(map) => Self::Object(
                map.iter()
                    .map(|(key, value)| {
                        let rule = if let Some(alias) =
                            value.as_str().filter(|s| map.contains_key(*s) && *s != key)
                        {
                            Self::Alias(alias.to_string())
                        } else {
                            Self::from_reference(value)
                        };
                        (key.clone(), rule)
                    })
                    .collect(),
            ),
            Value::Array(values) if values.is_empty() || values.iter().all(Value::is_object) => {
                Self::Array(
                    values
                        .iter()
                        .enumerate()
                        .map(|(index, v)| Selection {
                            index,
                            identity: ["id", "name", "_name", "path", "type"]
                                .iter()
                                .filter_map(|&k| {
                                    v.get(k)
                                        .filter(|v| v.is_string() || v.is_number())
                                        .map(|v| (k.to_string(), v.clone()))
                                })
                                .collect(),
                            shape: Self::from_reference(v),
                        })
                        .collect(),
                )
            }
            _ => Self::Keep,
        }
    }

    pub fn apply(&self, native: &mut Value) {
        match self {
            Self::Keep | Self::Alias(_) => {}
            Self::EmptyString if native.is_string() => *native = Value::String(String::new()),
            Self::Zero if native.is_number() => *native = serde_json::json!(0),
            Self::False if native.is_boolean() => *native = Value::Bool(false),
            Self::Object(rules) => {
                if let Some(map) = native.as_object_mut() {
                    let original = map.clone();
                    map.retain(|key, _| rules.contains_key(key));
                    for (key, rule) in rules {
                        if let Some(value) = map.get_mut(key) {
                            if let Self::Alias(other) = rule {
                                if let Some(original) = original.get(other) {
                                    *value = original.clone();
                                }
                            } else {
                                rule.apply(value);
                            }
                        }
                    }
                }
            }
            Self::Array(selections) => {
                if let Some(items) = native.as_array_mut() {
                    let original = std::mem::take(items);
                    let mut used = vec![false; original.len()];
                    for selection in selections {
                        let mut index = None;
                        // Stable IDs first, then names. Never transplant a new
                        // mod-only entity using its positional index.
                        for key in ["id", "name", "_name", "path"] {
                            if let Some(identity) = selection.identity.get(key) {
                                index = original.iter().enumerate().find_map(|(i, v)| {
                                    (!used[i]
                                        && v.get(key) == Some(identity)
                                        && selection
                                            .identity
                                            .get("type")
                                            .is_none_or(|t| v.get("type") == Some(t)))
                                    .then_some(i)
                                });
                                if index.is_some() {
                                    break;
                                }
                            }
                        }
                        if selection.identity.keys().all(|k| k == "type") {
                            let i = selection.index;
                            if i < original.len()
                                && !used[i]
                                && selection
                                    .identity
                                    .get("type")
                                    .is_none_or(|t| original[i].get("type") == Some(t))
                            {
                                index = Some(i);
                            }
                        }
                        if let Some(i) = index {
                            used[i] = true;
                            let mut value = original[i].clone();
                            selection.shape.apply(&mut value);
                            items.push(value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "rule", rename_all = "snake_case")]
pub enum Action {
    Empty,
    Json(Shape),
    Texture,
    Sprite,
    ReferenceSprite(SpritePlan),
    ReferenceTexture(TexturePlan),
    Unresolved(String),
    Native(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub path: String,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipe {
    pub version: u32,
    pub profile: String,
    pub provenance: String,
    pub targets: Vec<Target>,
}

pub fn safe_path(path: &str) -> bool {
    path.starts_with("data/")
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !is_device_name(part)
        })
}
pub fn is_device_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| e.to_string())?
        .trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .or_else(|_| json5::from_str(text))
        .map_err(|e| e.to_string())
}

fn is_culling_layout(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    [
        "lobbybackgroundpanelhd.json",
        "titlescreenpanelhd.json",
        "mainmenubuttonribbonhd.json",
        "loadscreenpanelhd.json",
        "characterstatspanelhd.json",
        "hirelinginventorypanelhd.json",
        "horadriccubelayouthd.json",
        "panelborderspanelhd.json",
        "partypanelhd.json",
        "playerinventoryexpansionlayouthd.json",
        "questlogpanelexpansionhd.json",
        "waypointspaneloriginalhd.json",
    ]
    .contains(&name)
}

fn native(storage: &casc_core::Storage, path: &str) -> Result<Option<Vec<u8>>, String> {
    match storage.read(&format!("data:{}", path.replace('/', "\\"))) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(casc_core::CascError::Backend {
            op: "CascOpenFile",
            code: 2,
        })
        | Err(casc_core::CascError::NotFound(_)) => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn scan(
    root: &Path,
    dir: &Path,
    targets: &mut Vec<Target>,
    storage: &casc_core::Storage,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err("源清单不接受符号链接".into());
        }
        let p = entry.path();
        if p.is_dir() {
            scan(root, &p, targets, storage)?;
            continue;
        }
        let relative = p
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if !relative.starts_with("data/") || relative == "data/global/dataversionbuild.txt" {
            continue;
        }
        if !safe_path(&relative) {
            return Err(format!("不安全的资源路径: {relative}"));
        }
        let len = entry.metadata().map_err(|e| e.to_string())?.len();
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let sentinel = len <= 3
            && matches!(
                fs::read(&p).map_err(|e| e.to_string())?.as_slice(),
                b"\xff\xfe" | b"\xfe\xff" | b"\xef\xbb\xbf"
            );
        let action = if len == 0 || sentinel {
            Action::Empty
        } else {
            match ext.as_str() {
                "texture" | "sprite" => {
                    let reference = fs::read(&p).map_err(|e| e.to_string())?;
                    if let Some(original) = native(storage, &relative)? {
                        if original == reference {
                            Action::Native("参考文件与当前原版一致，不新增覆盖".into())
                        } else if ext == "texture" {
                            match reference::derive_texture(&original,&reference) {
                                Ok(plan)=>Action::ReferenceTexture(plan),
                                Err(e) if reference.get(8..16)==Some(&[0x10,0x20,0x20,0x20,0x10,0x20,0x20,0x20]) => Action::Native(format!("参考纹理头损坏（尺寸字段被空格污染），无法确认替换效果，明确跳过：{e}")),
                                Err(e)=>Action::Unresolved(format!("纹理策略未确认: {e}")),
                            }
                        } else {
                            let lowend_path = relative
                                .strip_suffix(".sprite")
                                .filter(|_| !relative.ends_with(".lowend.sprite"))
                                .map(|p| format!("{p}.lowend.sprite"));
                            let lowend = lowend_path
                                .as_ref()
                                .map(|p| native(storage, p))
                                .transpose()?
                                .flatten();
                            let lowend = lowend_path.as_deref().zip(lowend.as_deref());
                            match reference::derive_sprite(&relative, &original, &reference, lowend)
                            {
                                Ok(plan) => Action::ReferenceSprite(plan),
                                Err(e) => Action::Unresolved(format!("sprite 策略未确认: {e}")),
                            }
                        }
                    } else {
                        Action::Native("参考目标在当前游戏中不存在，未生成外来素材".into())
                    }
                }
                "json" | "frontend" => {
                    if relative.contains("/ui/") && !is_culling_layout(&relative) {
                        Action::Native("使用原版 UI，排除自定义导航和计时器".into())
                    } else {
                        let bytes = fs::read(&p).map_err(|e| e.to_string())?;
                        match parse(&bytes) {
                            Ok(value) => Action::Json(Shape::from_reference(&value)),
                            Err(error) => {
                                Action::Native(format!("参考定义无法解析，未导入: {error}"))
                            }
                        }
                    }
                }
                _ => Action::Native("本版不复制非空第三方资源；此类型保持游戏原版".into()),
            }
        };
        targets.push(Target {
            path: relative,
            action,
        });
    }
    Ok(())
}

pub fn import(
    source: &Path,
    game: &Path,
    profile: &str,
    destination: &Path,
) -> Result<usize, String> {
    if !source.join("modinfo.json").is_file() {
        return Err("--source 请直接指定包含 modinfo.json 的 .mpq 目录".into());
    }
    let mut targets = Vec::new();
    let alias = crate::casc_path::CascStoragePath::prepare(game)?;
    let storage = casc_core::Storage::open(alias.as_path()).map_err(|e| e.to_string())?;
    scan(source, source, &mut targets, &storage)?;
    targets.sort_by(|a, b| a.path.cmp(&b.path));
    let count = targets.len();
    let recipe=Recipe{version:2,profile:profile.into(),provenance:"Per-target geometry, visible-region erase/fill, frame selection, native lowend substitutions and JSON culling derived from lowHD. No reference RGB buffers or game asset payloads included. https://www.nexusmods.com/diablo2resurrected/mods/1054".into(),targets};
    let bytes = serde_json::to_vec(&recipe).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    let mut gzip = flate2::write::GzEncoder::new(file, flate2::Compression::best());
    gzip.write_all(&bytes).map_err(|e| e.to_string())?;
    gzip.finish().map_err(|e| e.to_string())?;
    Ok(count)
}

pub fn decode(bytes: &[u8]) -> Result<Recipe, String> {
    let mut decoder = flate2::read::GzDecoder::new(bytes).take(64 * 1024 * 1024 + 1);
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|e| e.to_string())?;
    if decoded.len() > 64 * 1024 * 1024 {
        return Err("配方解压后过大".into());
    }
    let recipe: Recipe = serde_json::from_slice(&decoded).map_err(|e| e.to_string())?;
    if recipe.version != 2 {
        return Err("需要逐资源策略配方 v2；旧的统一缩放配方已停用".into());
    }
    let mut paths = std::collections::HashSet::new();
    for target in &recipe.targets {
        if !safe_path(&target.path) || !paths.insert(target.path.to_ascii_lowercase()) {
            return Err(format!("非法或重复配方路径: {}", target.path));
        }
        if let Action::ReferenceSprite(plan) = &target.action {
            if !safe_path(&plan.source) {
                return Err("非法 sprite 源路径".into());
            }
        }
    }
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_rules_do_not_copy_mod_values_or_new_components() {
        let reference = serde_json::json!({"name":"mod-only-name","entities":[{"name":"root","id":999,"components":[{"type":"Root","name":"root","state":""},{"type":"Custom","name":"injected"}]}]});
        let mut native = serde_json::json!({"name":"original","dependencies":{"models":["x"]},"entities":[{"name":"mesh","id":1},{"name":"root","id":2,"components":[{"type":"Root","name":"root","state":"old"},{"type":"Model","name":"mesh"}]}]});
        Shape::from_reference(&reference).apply(&mut native);
        assert_eq!(native["name"], "original");
        assert!(native.get("dependencies").is_none());
        assert_eq!(native["entities"][0]["id"], 2);
        assert_eq!(
            native["entities"][0]["components"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(native["entities"][0]["components"][0]["state"], "");
    }
    #[test]
    fn unsafe_paths_are_rejected() {
        for p in [
            "../file",
            "data/../file",
            "data/C:/file",
            "data/CON.json",
            "data/file.",
            "data//file",
            "data/\\file",
        ] {
            assert!(!safe_path(p), "{p}");
        }
        assert!(safe_path("data/hd/items/rune.json"));
    }
}
