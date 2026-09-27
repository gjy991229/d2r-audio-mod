use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "rule", rename_all = "snake_case")]
pub enum Action {
    Empty,
    NativeJson,
    NativeTexture,
    NativeSprite,
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

// Scope import reads only paths, file size and BOM sentinels. It does not read
// reference JSON definitions or image payloads into generation recipes.
fn scan(root: &Path, dir: &Path, targets: &mut Vec<Target>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err("源范围不接受符号链接".into());
        }
        let path = entry.path();
        if path.is_dir() {
            scan(root, &path, targets)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if !relative.starts_with("data/") || relative == "data/global/dataversionbuild.txt" {
            continue;
        }
        if !safe_path(&relative) {
            return Err(format!("非法目标路径 {relative}"));
        }
        let len = entry.metadata().map_err(|e| e.to_string())?.len();
        let sentinel = len <= 3
            && matches!(
                fs::read(&path).map_err(|e| e.to_string())?.as_slice(),
                b"\xff\xfe" | b"\xfe\xff" | b"\xef\xbb\xbf"
            );
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let action = if len == 0 || sentinel {
            Action::Empty
        } else {
            match ext.as_str() {
                "json" | "frontend" if relative.contains("/ui/") => {
                    Action::Native("独立规则保留原版 UI 布局".into())
                }
                "json" | "frontend" => Action::NativeJson,
                "texture" => Action::NativeTexture,
                "sprite" => Action::NativeSprite,
                _ => Action::Native("无独立处理规则，保留原版".into()),
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
    _game: &Path,
    profile: &str,
    destination: &Path,
) -> Result<usize, String> {
    if !source.join("modinfo.json").is_file() {
        return Err("--source 请直接指定包含 modinfo.json 的 .mpq 目录".into());
    }
    let mut targets = Vec::new();
    scan(source, source, &mut targets)?;
    targets.sort_by(|a, b| a.path.cmp(&b.path));
    let count = targets.len();
    let recipe=Recipe{version:4,profile:profile.into(),provenance:"Imported target boundary and blocking intent; no reference JSON parameters, image masks or geometry. Independent native-data processing rules.".into(),targets};
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
    if recipe.version != 4 {
        return Err("需要独立规则范围配方 v4；历史参考内容复现配方已停用".into());
    }
    let mut paths = std::collections::HashSet::new();
    for target in &recipe.targets {
        if !matches!(
            target.action,
            Action::Empty
                | Action::NativeJson
                | Action::NativeTexture
                | Action::NativeSprite
                | Action::Native(_)
        ) {
            return Err("独立规则配方不能包含参考参数、遮罩或复现指令".into());
        }
        if !safe_path(&target.path) || !paths.insert(target.path.to_ascii_lowercase()) {
            return Err(format!("非法或重复配方路径: {}", target.path));
        }
    }
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
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
