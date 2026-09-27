//! Frozen b12 generation recipes. Contains configuration edits and geometry,
//! never image/particle payloads. Runtime reads only the executable and CASC.
use super::{compatible, Report, Request, Stage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const GAME_VERSION_PATH: &str = "data/global/dataversionbuild.txt";

#[derive(Serialize, Deserialize)]
struct Recipe {
    version: u32,
    profile: String,
    baseline: String,
    entries: Vec<Entry>,
    omitted: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    output_sha256: String,
    #[serde(flatten)]
    rule: Rule,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Rule {
    GameVersion,
    Empty,
    Native {
        source: String,
        input_sha256: String,
    },
    Texture {
        source: String,
        input_sha256: String,
        max_side: usize,
    },
    Sprite {
        source: String,
        input_sha256: String,
        geometry: [usize; 4],
    },
    Json {
        source: String,
        input_sha256: String,
        edits: Vec<Edit>,
    },
    Text {
        source: String,
        input_sha256: String,
        start: usize,
        end: usize,
        insert: String,
    },
    LiteralText {
        text: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Edit {
    Set { path: Vec<String>, value: Value },
    Remove { path: Vec<String> },
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_path(path: &str) -> bool {
    !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(['.', ' '])
                && !p.chars().any(|c| c.is_control() || "<>\"|?*".contains(c))
                && {
                    let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
                    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                        && !(stem.len() == 4
                            && (stem.starts_with("COM") || stem.starts_with("LPT"))
                            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
                }
        })
}
fn text_path(path: &str) -> bool {
    ["json", "frontend", "txt"].contains(&path.rsplit('.').next().unwrap_or(""))
}
impl Rule {
    fn source(&self) -> Option<(&str, &str)> {
        match self {
            Self::Native {
                source,
                input_sha256,
            }
            | Self::Texture {
                source,
                input_sha256,
                ..
            }
            | Self::Sprite {
                source,
                input_sha256,
                ..
            }
            | Self::Json {
                source,
                input_sha256,
                ..
            }
            | Self::Text {
                source,
                input_sha256,
                ..
            } => Some((source, input_sha256)),
            _ => None,
        }
    }
    fn method(&self) -> &'static str {
        match self {
            Self::GameVersion => "game_data_version_passthrough",
            Self::Empty => "empty_override",
            Self::Native { .. } => "game_native",
            Self::Texture { .. } => "game_texture_mip",
            Self::Sprite { .. } => "game_sprite_geometry",
            Self::Json { .. } => "game_json_bundled_edits",
            Self::Text { .. } => "game_text_bundled_edit",
            Self::LiteralText { .. } => "bundled_text_configuration",
        }
    }
}
fn validate(recipe: &Recipe, profile: &str) -> Result<(), String> {
    if recipe.version != 1 || recipe.profile != profile || recipe.baseline != "b12" {
        return Err("内置方案版本/配置不匹配".into());
    }
    let mut names = BTreeSet::new();
    for e in &recipe.entries {
        if (e.path == GAME_VERSION_PATH) != matches!(e.rule, Rule::GameVersion) {
            return Err("游戏数据版本必须直接取自本机原版".into());
        }
        if !safe_path(&e.path) || !names.insert(e.path.to_ascii_lowercase()) {
            return Err(format!("不安全或重复的内置路径：{}", e.path));
        }
        if matches!(
            e.rule,
            Rule::LiteralText { .. } | Rule::Text { .. } | Rule::Json { .. }
        ) && !text_path(&e.path)
        {
            return Err("内置规则禁止保存二进制素材".into());
        }
        if let Some((source, _)) = e.rule.source() {
            if !source.starts_with("data/") || !safe_path(source) {
                return Err("非法 CASC 来源".into());
            }
        }
    }
    if !names.contains("modinfo.json") || !names.contains(GAME_VERSION_PATH) {
        return Err("内置方案缺少 modinfo 或游戏数据版本文件".into());
    }
    for path in &recipe.omitted {
        if !safe_path(path) || !names.insert(path.to_ascii_lowercase()) {
            return Err("非法省略项".into());
        }
    }
    Ok(())
}
fn load(profile: &str) -> Result<Recipe, String> {
    let data: &[u8] = match profile {
        "min" => include_bytes!("../../resources/lightweight/b13/min.json.gz"),
        "filler" => include_bytes!("../../resources/lightweight/b13/filler.json.gz"),
        "main" => include_bytes!("../../resources/lightweight/b13/main.json.gz"),
        _ => return Err("未知内置方案".into()),
    };
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("内置方案过大".into());
    }
    let recipe: Recipe = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&recipe, profile)?;
    Ok(recipe)
}
fn apply(value: &mut Value, edit: &Edit) -> Result<(), String> {
    let path = match edit {
        Edit::Set { path, .. } | Edit::Remove { path } => path,
    };
    if path.is_empty() {
        if let Edit::Set {
            value: replacement, ..
        } = edit
        {
            *value = replacement.clone();
            return Ok(());
        }
        return Err("不能删除 JSON 根".into());
    }
    let mut parent = value;
    for key in &path[..path.len() - 1] {
        parent = match parent {
            Value::Object(map) => map.get_mut(key),
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
            _ => None,
        }
        .ok_or_else(|| format!("JSON 规则父路径不存在：{path:?}"))?;
    }
    let key = path.last().unwrap();
    match (parent, edit) {
        (Value::Object(map), Edit::Set { value, .. }) => {
            map.insert(key.clone(), value.clone());
        }
        (Value::Object(map), Edit::Remove { .. }) => {
            map.remove(key).ok_or("JSON 删除目标不存在")?;
        }
        (Value::Array(items), Edit::Set { value, .. }) => {
            let index = key.parse::<usize>().map_err(|_| "非法数组索引")?;
            *items.get_mut(index).ok_or("数组索引越界")? = value.clone();
        }
        _ => return Err("JSON 规则目标类型不匹配".into()),
    }
    Ok(())
}
fn read(storage: &casc_core::Storage, source: &str) -> Result<Vec<u8>, String> {
    storage
        .read(&format!("data:{}", source.replace('/', "\\")))
        .map_err(|e| format!("读取原版 {source}：{e}"))
}
fn generate(e: &Entry, storage: &casc_core::Storage) -> Result<Vec<u8>, String> {
    generate_from(e, |source| read(storage, source))
}
fn generate_from(
    e: &Entry,
    mut read_native: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    if matches!(e.rule, Rule::GameVersion) {
        if e.path != GAME_VERSION_PATH {
            return Err("非法版本号规则路径".into());
        }
        let bytes = read_native(GAME_VERSION_PATH)?;
        let version = std::str::from_utf8(&bytes)
            .map_err(|_| "原版数据版本不是文本")?
            .trim_start_matches('\u{feff}')
            .trim();
        if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
            return Err("原版数据版本号无效".into());
        }
        return Ok(bytes);
    }
    let input = if let Some((source, expected)) = e.rule.source() {
        let bytes = read_native(source)?;
        if hash(&bytes) != expected {
            return Err(format!(
                "原版资源版本变化：{source}；需更新内置规则，未发布不一致产物"
            ));
        }
        bytes
    } else {
        Vec::new()
    };
    let out = match &e.rule {
        Rule::GameVersion => unreachable!(),
        Rule::Empty => Vec::new(),
        Rule::Native { .. } => input,
        Rule::Texture { max_side, .. } => super::assets::texture(&input, *max_side)?,
        Rule::Sprite { geometry, .. } => compatible::sprite_geometry(&input, *geometry)?,
        Rule::Json { edits, .. } => {
            let mut value = super::parse(&input)?;
            for edit in edits {
                apply(&mut value, edit)?;
            }
            serde_json::to_vec(&value).map_err(|e| e.to_string())?
        }
        Rule::Text {
            start, end, insert, ..
        } => {
            let native = std::str::from_utf8(&input).map_err(|e| e.to_string())?;
            if start > end || !native.is_char_boundary(*start) || !native.is_char_boundary(*end) {
                return Err("文本规则边界不匹配".into());
            }
            format!("{}{}{}", &native[..*start], insert, &native[*end..]).into_bytes()
        }
        Rule::LiteralText { text } => text.as_bytes().to_vec(),
    };
    if hash(&out) != e.output_sha256 {
        return Err(format!("与 b12 基线不一致：{}", e.path));
    }
    Ok(out)
}
pub(super) fn build(
    r: Request,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Report, String> {
    if r.source.is_some() {
        return Err(
            "独立生成不读取 --source；只需 --game。原包对照请显式使用 --mode template".into(),
        );
    }
    let recipe = load(&r.profile)?;
    let alias = crate::casc_path::CascStoragePath::prepare(&r.game)?;
    let storage = casc_core::Storage::open(alias.as_path()).map_err(|e| e.to_string())?;
    let base = r
        .name
        .unwrap_or_else(|| super::default_name(&r.profile).into());
    if !super::valid_name(&base) {
        return Err("不安全的 MOD 名称".into());
    }
    let parent = r.output.unwrap_or_else(|| r.game.join("mods"));
    let data = r
        .game
        .join("Data")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    for ancestor in parent.ancestors() {
        if ancestor.canonicalize().is_ok_and(|p| p.starts_with(&data)) {
            return Err("不能输出到游戏 Data 内部".into());
        }
    }
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    let (mut name, mut suffix) = (base.clone(), 2);
    while parent.join(&name).exists() {
        name = format!("{base}-{suffix}");
        suffix += 1;
    }
    let destination = parent.join(&name);
    let mut stage = Stage {
        path: parent.join(format!(".hub-building-{}", uuid::Uuid::new_v4())),
        parent,
        committed: false,
    };
    fs::create_dir(&stage.path).map_err(|e| e.to_string())?;
    let mpq = stage.path.join(format!("{name}.mpq"));
    let (mut counts, mut origins, mut generated_bytes, mut expected) =
        (BTreeMap::new(), Vec::new(), 0u64, BTreeMap::new());
    let total = recipe.entries.len();
    let mut game_data_version = None;
    for (i, e) in recipe.entries.iter().enumerate() {
        if i % 200 == 0 {
            progress(i, total * 2, "从原版按内置规则生成");
        }
        let mut bytes = generate(e, &storage)?;
        if e.path == GAME_VERSION_PATH {
            game_data_version = Some(
                String::from_utf8(bytes.clone())
                    .map_err(|e| e.to_string())?
                    .trim_start_matches('\u{feff}')
                    .trim()
                    .to_owned(),
            );
        }
        if e.path == "modinfo.json" {
            bytes = super::rename_info(&bytes, &name)?;
        }
        expected.insert(PathBuf::from(&e.path), hash(&bytes));
        super::write(&mpq.join(&e.path), &bytes)?;
        generated_bytes += bytes.len() as u64;
        *counts.entry(e.rule.method().into()).or_default() += 1;
        if bytes.is_empty() {
            *counts.entry("empty_files".into()).or_default() += 1;
        }
        if e.path != "modinfo.json" {
            origins.push(compatible::Origin {
                path: e.path.clone(),
                method: e.rule.method().into(),
                game_source: if e.path == GAME_VERSION_PATH {
                    Some(GAME_VERSION_PATH.into())
                } else {
                    e.rule.source().map(|(p, _)| p.into())
                },
                equality: if e.path == GAME_VERSION_PATH {
                    "current_native_bytes_equal"
                } else {
                    "embedded_recipe_sha256_equal"
                }
                .into(),
            });
        }
    }
    let mut actual = Vec::new();
    super::list(&mpq, &mpq, &mut actual)?;
    actual.sort();
    if actual != expected.keys().cloned().collect::<Vec<_>>() {
        return Err("输出文件集合与内置方案不一致".into());
    }
    for (i, (path, digest)) in expected.iter().enumerate() {
        if i % 200 == 0 {
            progress(total + i, total * 2, "逐文件核验 b12 校验值");
        }
        if hash(&fs::read(mpq.join(path)).map_err(|e| e.to_string())?) != *digest {
            return Err(format!("输出损坏：{}", path.display()));
        }
    }
    for path in &recipe.omitted {
        origins.push(compatible::Origin {
            path: path.clone(),
            method: "b12_omitted_asset".into(),
            game_source: None,
            equality: "not_emitted_same_as_b12".into(),
        });
    }
    counts.insert("verified_files".into(), total);
    counts.insert("b12_omitted_assets".into(), recipe.omitted.len());
    let txt = expected.contains_key(Path::new("data/global/excel/missiles.txt"));
    let report = Report {
        producer: "d2r-native-bundled-generator".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        mode: "bundled_rebuild".into(),
        profile: r.profile,
        source_directory: format!("embedded:hub/{}", recipe.profile),
        mod_name: name.clone(),
        mod_directory: destination.to_string_lossy().into_owned(),
        launch_arguments: format!("-mod {name}{}", if txt { " -txt" } else { "" }),
        counts,
        generated_bytes,
        verified_compatible: false,
        verified_output_integrity: true,
        origins,
        verified_identical_except_modinfo: false,
        game_data_version,
        verified_b12_except_name_and_data_version: false,
        runtime_verified: false,
    };
    super::write(
        &stage.path.join("generation-manifest.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )?;
    super::write(&stage.path.join("README.txt"), format!("{}\r\n启动：{}\r\n生成器版本：{}\r\n游戏数据版本：{}\r\n由内置规则与本机游戏原版资源生成，文件完整性已核验。\r\n保留低清素材与当前显示比例；暂停菜单支持手动保存退出。\r\n逐文件来源与校验结果见 generation-manifest.json。\r\n", report.mod_name, report.launch_arguments, report.producer_version, report.game_data_version.as_deref().unwrap_or("unknown")).as_bytes())?;
    if destination.exists() {
        return Err("输出名称被占用".into());
    }
    fs::rename(&stage.path, &destination).map_err(|e| e.to_string())?;
    stage.committed = true;
    progress(total * 2, total * 2, "完成：与 b12 基线一致");
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_version_tracks_native_storage_instead_of_frozen_baseline() {
        let e = Entry {
            path: GAME_VERSION_PATH.into(),
            output_sha256: String::new(),
            rule: Rule::GameVersion,
        };
        for native in [b"93854".as_slice(), b"99999\r\n".as_slice()] {
            assert_eq!(
                generate_from(&e, |path| {
                    assert_eq!(path, GAME_VERSION_PATH);
                    Ok(native.to_vec())
                })
                .unwrap(),
                native
            );
        }
        assert!(generate_from(&e, |_| Err("missing version".into())).is_err());
        assert!(generate_from(&e, |_| Ok(b"".to_vec())).is_err());
        assert!(generate_from(&e, |_| Ok(b"not a version".to_vec())).is_err());
    }
    #[test]
    fn changed_native_or_changed_output_is_rejected_without_fallback() {
        let original = b"native bytes";
        let mut e = Entry {
            path: "data/image.sprite".into(),
            output_sha256: hash(original),
            rule: Rule::Native {
                source: "data/image.sprite".into(),
                input_sha256: hash(original),
            },
        };
        assert_eq!(
            generate_from(&e, |_| Ok(original.to_vec())).unwrap(),
            original
        );
        assert!(generate_from(&e, |_| Ok(b"updated game".to_vec()))
            .unwrap_err()
            .contains("版本变化"));
        e.output_sha256 = hash(b"wrong expected output");
        assert!(generate_from(&e, |_| Ok(original.to_vec()))
            .unwrap_err()
            .contains("b12"));
        let empty = Entry {
            path: "data/blocked.json".into(),
            output_sha256: hash(b""),
            rule: Rule::Empty,
        };
        assert!(
            generate_from(&empty, |_| panic!("empty rules must not read assets"))
                .unwrap()
                .is_empty()
        );
    }
    fn diff(a: &Value, b: &Value, path: Vec<String>, edits: &mut Vec<Edit>) {
        if a == b {
            return;
        }
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => {
                for key in a.keys().filter(|k| !b.contains_key(*k)) {
                    let mut p = path.clone();
                    p.push(key.clone());
                    edits.push(Edit::Remove { path: p });
                }
                for (key, value) in b {
                    let mut p = path.clone();
                    p.push(key.clone());
                    if let Some(old) = a.get(key) {
                        diff(old, value, p, edits)
                    } else {
                        edits.push(Edit::Set {
                            path: p,
                            value: value.clone(),
                        });
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    let mut p = path.clone();
                    p.push(i.to_string());
                    diff(a, b, p, edits);
                }
            }
            _ => edits.push(Edit::Set {
                path,
                value: b.clone(),
            }),
        }
    }
    fn text_edit(a: &str, b: &str) -> (usize, usize, String) {
        let mut start = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
        while !a.is_char_boundary(start) || !b.is_char_boundary(start) {
            start -= 1;
        }
        let mut suffix = a.as_bytes()[start..]
            .iter()
            .rev()
            .zip(b.as_bytes()[start..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        while !a.is_char_boundary(a.len() - suffix) || !b.is_char_boundary(b.len() - suffix) {
            suffix -= 1;
        }
        (start, a.len() - suffix, b[start..b.len() - suffix].into())
    }
    #[test]
    fn edits_preserve_arrays_deletions_numbers_and_unicode() {
        let mut a = serde_json::json!({"entities":[{"a":1,"remove":2}],"old":[1,2]});
        let b = serde_json::json!({"entities":[{"a":1.5,"new":"测试"}],"old":[]});
        let mut edits = vec![];
        diff(&a, &b, vec![], &mut edits);
        for e in &edits {
            apply(&mut a, e).unwrap();
        }
        assert_eq!(a, b);
        for (a, b) in [("原版😀\r\nabc", "原版😁\nxyz"), ("abc", ""), ("", "测试")] {
            let (s, e, t) = text_edit(a, b);
            assert_eq!(format!("{}{}{}", &a[..s], t, &a[e..]), b);
        }
        assert!(!safe_path("../escape"));
        assert!(!safe_path("data/C:bad"));
        assert!(!safe_path("data/NUL.json"));
    }
    #[test]
    fn embedded_profiles_contain_no_binary_payload_rules() {
        for profile in ["min", "filler", "main"] {
            load(profile).unwrap();
        }
    }
    #[test]
    fn recipe_cannot_omit_game_version_and_product_metadata_is_neutral() {
        for profile in ["min", "filler", "main"] {
            let mut recipe = load(profile).unwrap();
            let info = recipe
                .entries
                .iter()
                .find(|e| e.path == "modinfo.json")
                .unwrap();
            let Rule::LiteralText { text } = &info.rule else {
                panic!("metadata must be text");
            };
            let value: Value = serde_json::from_str(text).unwrap();
            assert_eq!(value["name"], super::super::default_name(profile));
            assert_eq!(value["savepath"], "../");
            assert_eq!(value.as_object().unwrap().len(), 2);
            recipe.entries.retain(|e| e.path != GAME_VERSION_PATH);
            assert!(validate(&recipe, profile).is_err());
        }
    }
    /// Development-only compiler. Requires explicit local b12 artifacts; never
    /// compiled into the released executable.
    #[test]
    #[ignore = "authoring only: requires D2R_FREEZE_GAME and existing verified b12 mods"]
    fn freeze_b12_recipes() {
        use std::io::Write;
        let game = PathBuf::from(std::env::var_os("D2R_FREEZE_GAME").expect("D2R_FREEZE_GAME"));
        let storage = casc_core::Storage::open(&game).unwrap();
        for profile in ["min", "filler", "main"] {
            let name = format!("D2RNative-{profile}-b12");
            let root = game.join("mods").join(&name);
            let mpq = root.join(format!("{name}.mpq"));
            let manifest: Value =
                serde_json::from_slice(&fs::read(root.join("generation-manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["producer_version"], "1.4.0-beta.12");
            let origins: BTreeMap<_, _> = manifest["origins"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| (o["path"].as_str().unwrap(), o))
                .collect();
            let mut paths = vec![];
            super::super::list(&mpq, &mpq, &mut paths).unwrap();
            paths.sort();
            let mut entries = vec![];
            for rel in paths {
                let path = rel.to_string_lossy().replace('\\', "/");
                let mut output = fs::read(mpq.join(&rel)).unwrap();
                if path == "modinfo.json" {
                    output = serde_json::to_vec_pretty(&serde_json::json!({"name":super::super::default_name(profile),"savepath":"../"})).unwrap();
                } else if path == "optimal_settings/readme.txt" {
                    output = "此文件仅为可选画质参数参考。生成器不会自动应用这些设置，画质可按需求在游戏设置中调整。\n".as_bytes().to_vec();
                }
                let origin = origins.get(path.as_str());
                let method = origin.and_then(|v| v["method"].as_str()).unwrap_or("");
                let source = origin
                    .and_then(|v| v["game_source"].as_str())
                    .unwrap_or(&path)
                    .to_string();
                let native = if !output.is_empty() && source.starts_with("data/") {
                    match storage.read(&format!("data:{}", source.replace('/', "\\"))) {
                        Ok(b) => Some(b),
                        Err(casc_core::CascError::Backend {
                            op: "CascOpenFile",
                            code: 2,
                        })
                        | Err(casc_core::CascError::NotFound(_)) => None,
                        Err(e) => panic!("{e}"),
                    }
                } else {
                    None
                };
                let rule = if path == GAME_VERSION_PATH {
                    Rule::GameVersion
                } else if output.is_empty() {
                    Rule::Empty
                } else if let Some(native) = native {
                    let input_sha256 = hash(&native);
                    if native == output {
                        Rule::Native {
                            source,
                            input_sha256,
                        }
                    } else if method == "game_texture_mip" {
                        let w = u32::from_le_bytes(output[8..12].try_into().unwrap()) as usize;
                        let h = u32::from_le_bytes(output[12..16].try_into().unwrap()) as usize;
                        Rule::Texture {
                            source,
                            input_sha256,
                            max_side: w.max(h).next_power_of_two(),
                        }
                    } else if method == "game_sprite_geometry_only"
                        || method == "game_lowend_geometry_only"
                    {
                        let u =
                            |p| u32::from_le_bytes(output[p..p + 4].try_into().unwrap()) as usize;
                        Rule::Sprite {
                            source,
                            input_sha256,
                            geometry: [
                                u(8),
                                u(12),
                                u(20),
                                u16::from_le_bytes(output[6..8].try_into().unwrap()) as usize,
                            ],
                        }
                    } else if method.starts_with("game_json_reference_rules") {
                        let a = super::super::parse(&native).unwrap();
                        let b: Value = serde_json::from_slice(&output).unwrap();
                        let mut edits = vec![];
                        diff(&a, &b, vec![], &mut edits);
                        Rule::Json {
                            source,
                            input_sha256,
                            edits,
                        }
                    } else {
                        assert!(text_path(&path), "binary payload forbidden {path} {method}");
                        let (start, end, insert) = text_edit(
                            std::str::from_utf8(&native).unwrap(),
                            std::str::from_utf8(&output).unwrap(),
                        );
                        Rule::Text {
                            source,
                            input_sha256,
                            start,
                            end,
                            insert,
                        }
                    }
                } else {
                    assert!(text_path(&path), "missing native binary {path}");
                    Rule::LiteralText {
                        text: String::from_utf8(output.clone()).unwrap(),
                    }
                };
                let entry = Entry {
                    path: path.clone(),
                    output_sha256: if path == GAME_VERSION_PATH {
                        String::new()
                    } else {
                        hash(&output)
                    },
                    rule,
                };
                assert_eq!(
                    generate(&entry, &storage).unwrap(),
                    if entry.path == GAME_VERSION_PATH {
                        read(&storage, GAME_VERSION_PATH).unwrap()
                    } else {
                        output
                    }
                );
                entries.push(entry);
            }
            let omitted = origins
                .iter()
                .filter(|(_, o)| o["method"] == "omitted_missing_native_asset")
                .map(|(p, _)| p.to_string())
                .collect();
            let recipe = Recipe {
                version: 1,
                profile: profile.into(),
                baseline: "b12".into(),
                entries,
                omitted,
            };
            validate(&recipe, profile).unwrap();
            let json = serde_json::to_vec(&recipe).unwrap();
            let mut gzip = flate2::GzBuilder::new()
                .mtime(0)
                .write(Vec::new(), flate2::Compression::best());
            gzip.write_all(&json).unwrap();
            let out = gzip.finish().unwrap();
            fs::write(format!("resources/lightweight/b13/{profile}.json.gz"), &out).unwrap();
            println!(
                "{profile}: {} entries, {} compressed bytes",
                recipe.entries.len(),
                out.len()
            );
        }
    }
}
