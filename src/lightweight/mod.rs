mod assets;
mod memory_policy;
mod native_policy;
pub mod recipe;

use crate::casc_path::CascStoragePath;
use recipe::{Action, Recipe};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct Request {
    pub game: PathBuf,
    pub output: Option<PathBuf>,
    pub name: Option<String>,
    pub profile: String,
    pub texture_size: usize,
    pub sprite_scale: usize,
    pub disable_scoped_particles: bool,
    pub recipe_file: Option<PathBuf>,
    pub asset_types: Vec<String>,
}

#[derive(Serialize)]
pub struct Outcome {
    pub path: String,
    pub action: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct Report {
    pub producer: String,
    pub producer_version: String,
    pub recipe_version: u32,
    pub profile: String,
    pub provenance: String,
    pub mod_name: String,
    pub mod_directory: String,
    pub launch_arguments: String,
    pub game_build: String,
    pub texture_size: usize,
    pub sprite_scale: usize,
    pub disable_scoped_particles: bool,
    pub counts: BTreeMap<String, usize>,
    pub original_transformed_bytes: u64,
    pub generated_asset_bytes: u64,
    pub details: Vec<Outcome>,
    pub runtime_verified: bool,
}

pub fn builtin(profile: &str) -> Result<Recipe, String> {
    let bytes: &[u8] = match profile {
        "min" => include_bytes!("../../resources/lightweight/min.json.gz"),
        "filler" => include_bytes!("../../resources/lightweight/filler.json.gz"),
        "main" => include_bytes!("../../resources/lightweight/main.json.gz"),
        _ => return Err("轻量化 profile 仅支持 min、filler、main".into()),
    };
    recipe::decode(bytes)
}

struct Staging {
    path: PathBuf,
    parent: PathBuf,
    committed: bool,
}
impl Drop for Staging {
    fn drop(&mut self) {
        if !self.committed {
            // Only remove the fresh UUID transaction directory we own.
            if let Ok(actual) = self.path.canonicalize() {
                if actual.parent() == Some(self.parent.as_path())
                    && actual
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(".lightweight-building-"))
                {
                    let _ = fs::remove_dir_all(actual);
                }
            }
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("资源路径没有父目录")?).map_err(|e| e.to_string())?;
    fs::write(path, bytes).map_err(|e| format!("写入 {} 失败: {e}", path.display()))
}
fn read(storage: &casc_core::Storage, path: &str) -> Result<Vec<u8>, casc_core::CascError> {
    storage.read(&format!("data:{}", path.replace('/', "\\")))
}
fn missing(error: &casc_core::CascError) -> bool {
    matches!(
        error,
        casc_core::CascError::Backend {
            op: "CascOpenFile",
            code: 2
        } | casc_core::CascError::NotFound(_)
    )
}
fn note(report: &mut Report, path: &str, action: &str, reason: String) {
    *report.counts.entry(action.into()).or_default() += 1;
    report.details.push(Outcome {
        path: path.into(),
        action: action.into(),
        reason,
    });
}

pub fn build(
    request: Request,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Report, String> {
    if ![0, 1, 2, 4, 8, 16, 32].contains(&request.texture_size) {
        return Err("纹理尺寸使用 0（自动4），或显式指定 1、2、4、8、16、32".into());
    }
    if request.sprite_scale != 1 {
        return Err("UI 布局未同步缩放，非空 sprite 只支持 --sprite-scale 1（原版尺寸）".into());
    }
    if request.disable_scoped_particles && !request.asset_types.iter().any(|s| s == "empty") {
        return Err("--effects off 需要包含 empty 类型，才能屏蔽粒子资源".into());
    }
    if request.asset_types.is_empty()
        || request
            .asset_types
            .iter()
            .any(|s| !["empty", "json", "texture", "sprite"].contains(&s.as_str()))
    {
        return Err("--asset-types 必须是 empty,json,texture,sprite 中的一项或多项".into());
    }
    let recipe = if let Some(path) = &request.recipe_file {
        recipe::decode(&fs::read(path).map_err(|e| e.to_string())?)?
    } else {
        builtin(&request.profile)?
    };
    let name = request
        .name
        .unwrap_or_else(|| format!("D2RLight-{}", recipe.profile));
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || recipe::is_device_name(&name)
    {
        return Err("MOD 名称必须为 1–64 个 ASCII 字母、数字、- 或 _，不能使用系统保留名称".into());
    }
    if !request.game.join(".build.info").is_file() || !request.game.join("Data").is_dir() {
        return Err("请选择含 .build.info 和 Data 的 D2R 游戏目录".into());
    }
    let alias = CascStoragePath::prepare(&request.game)?;
    let storage = casc_core::Storage::open(alias.as_path()).map_err(|e| e.to_string())?;
    let shared_biome =
        recipe.targets.iter().any(|t| {
            t.path == memory_policy::SHARED_BIOME && matches!(t.action, Action::NativeJson)
        }) && request.asset_types.iter().any(|s| s == "json");
    if shared_biome {
        read(&storage, memory_policy::BIOME_BASE)
            .map_err(|e| format!("简化 biome 原版模板缺失：{e}"))?;
        read(&storage, memory_policy::DEFAULT_VIS)
            .map_err(|e| format!("原版默认环境定义缺失：{e}"))?;
    }
    let mut terrain_support = Vec::new();
    if shared_biome {
        let bytes = read(&storage, memory_policy::BIOME_BASE).map_err(|e| e.to_string())?;
        let biome = recipe::parse(&bytes)?;
        let layer = biome
            .pointer("/terrainDataLow/terrainLayers/0")
            .ok_or("原版 biome 缺少低画质地形层，未猜测占位纹理")?;
        for channel in memory_policy::TERRAIN_CHANNELS {
            let source = layer
                .get(channel)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("原版地形层缺少 {channel}"))?;
            let source = native_policy::resource_path(source);
            let original =
                read(&storage, &source).map_err(|e| format!("读取地形占位基线 {source}: {e}"))?;
            let small = assets::texture(&original, 1)?;
            terrain_support.push((
                memory_policy::tiny_terrain_path(channel),
                small,
                source,
                original.len(),
            ));
        }
    }
    let version = read(&storage, "data/global/dataversionbuild.txt").map_err(|e| e.to_string())?;
    let parent = request.output.unwrap_or_else(|| request.game.join("mods"));
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    let game_data = request
        .game
        .join("Data")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if parent.starts_with(game_data) {
        return Err("不能向游戏 Data 存储目录写入 MOD".into());
    }
    let mut final_name = name.clone();
    let mut n = 2;
    while parent.join(&final_name).exists() {
        final_name = format!("{name}-{n}");
        n += 1;
    }
    let destination = parent.join(&final_name);
    let stage_path = parent.join(format!(".lightweight-building-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage_path).map_err(|e| e.to_string())?;
    let mut staging = Staging {
        path: stage_path,
        parent: parent.clone(),
        committed: false,
    };
    let mpq = staging.path.join(format!("{final_name}.mpq"));
    let mut report = Report {
        producer: "d2r-lightweight-generator".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        recipe_version: recipe.version,
        profile: recipe.profile,
        provenance: recipe.provenance,
        mod_name: final_name.clone(),
        mod_directory: destination.to_string_lossy().into_owned(),
        launch_arguments: format!("-mod {final_name}"),
        game_build: String::from_utf8(version.clone())
            .map_err(|e| e.to_string())?
            .trim()
            .into(),
        texture_size: request.texture_size,
        sprite_scale: request.sprite_scale,
        disable_scoped_particles: request.disable_scoped_particles,
        counts: BTreeMap::new(),
        original_transformed_bytes: 0,
        generated_asset_bytes: 0,
        details: Vec::new(),
        runtime_verified: false,
    };
    for (path, bytes, source, size) in terrain_support {
        write(&mpq.join(&path), &bytes)?;
        report.original_transformed_bytes += size as u64;
        report.generated_asset_bytes += bytes.len() as u64;
        note(
            &mut report,
            &path,
            "required_terrain_texture",
            format!("共享简化 biome 必需的 1×1 地形材质，来自原版 {source}"),
        );
    }
    let mut disabled: std::collections::HashSet<String> = recipe
        .targets
        .iter()
        .filter(|t| matches!(t.action, Action::Empty))
        .map(|t| native_policy::resource_path(&t.path))
        .collect();
    let mut effects = std::collections::HashSet::new();
    if request.disable_scoped_particles {
        for (index, target) in recipe.targets.iter().enumerate() {
            if target.path.ends_with(".particles") {
                effects.insert(target.path.clone());
            }
            if !matches!(target.action, Action::NativeJson) {
                continue;
            }
            if index % 100 == 0 {
                progress(index, recipe.targets.len(), "检查范围内 JSON 的粒子依赖");
            }
            match read(&storage, &target.path) {
                Ok(bytes) => {
                    let value = recipe::parse(&bytes)
                        .map_err(|e| format!("粒子依赖读取失败 {}: {e}", target.path))?;
                    native_policy::particle_references(&value, &mut effects);
                }
                Err(e) if missing(&e) => {}
                Err(e) => return Err(format!("读取粒子依赖失败 {}: {e}", target.path)),
            }
        }
        let existing: std::collections::HashSet<_> =
            recipe.targets.iter().map(|t| t.path.as_str()).collect();
        let mut sorted = effects.iter().cloned().collect::<Vec<_>>();
        sorted.sort();
        for path in sorted {
            if existing.contains(path.as_str()) {
                continue;
            }
            match read(&storage, &path) {
                Ok(_) => {
                    write(&mpq.join(&path), &[])?;
                    note(
                        &mut report,
                        &path,
                        "derived_particle_off",
                        "来自范围内原版 JSON 的直接粒子依赖；仅增加空覆盖".into(),
                    );
                }
                Err(e) if missing(&e) => {
                    effects.remove(&path);
                }
                Err(e) => return Err(format!("粒子资源读取失败 {path}: {e}")),
            }
        }
        disabled.extend(effects.iter().cloned());
    }
    let total = recipe.targets.len();
    for (index, target) in recipe.targets.iter().enumerate() {
        if index % 100 == 0 {
            progress(index, total, &target.path);
        }
        if effects.contains(&target.path) {
            write(&mpq.join(&target.path), &[])?;
            note(
                &mut report,
                &target.path,
                "scoped_particle_off",
                "关闭本方案范围内的粒子资源".into(),
            );
            continue;
        }
        if matches!(target.action, Action::NativeTexture)
            && native_policy::is_vfx_texture(&target.path)
        {
            note(
                &mut report,
                &target.path,
                "vfx_native_texture",
                "保留原版特效纹理，避免极低分辨率破坏透明度和形状".into(),
            );
            continue;
        }
        let category = match target.action {
            Action::Empty => "empty",
            Action::NativeJson => "json",
            Action::NativeTexture => "texture",
            Action::NativeSprite => "sprite",
            Action::Native(_) => "native",
        };
        if let Action::Native(reason) = &target.action {
            note(&mut report, &target.path, "kept_native", reason.clone());
            continue;
        }
        if !request.asset_types.iter().any(|s| s == category) {
            continue;
        }
        if matches!(target.action, Action::Empty) {
            write(&mpq.join(&target.path), &[])?;
            *report.counts.entry("empty".into()).or_default() += 1;
            continue;
        }
        if request.sprite_scale == 1 && matches!(target.action, Action::NativeSprite) {
            note(
                &mut report,
                &target.path,
                "user_native",
                "保留原版 UI sprite 尺寸和布局对应关系".into(),
            );
            continue;
        }
        let source_path = if target.path == memory_policy::SHARED_BIOME && shared_biome {
            memory_policy::BIOME_BASE
        } else {
            target.path.as_str()
        };
        let original = match read(&storage, source_path) {
            Ok(bytes) => bytes,
            Err(e) if missing(&e) => {
                note(&mut report, &target.path, "missing_native", e.to_string());
                continue;
            }
            Err(e) => return Err(format!("游戏资源读取失败 {}: {e}", target.path)),
        };
        let generated = match &target.action {
            Action::NativeJson => {
                let mut value = recipe::parse(&original)
                    .map_err(|e| format!("原版 JSON 无法解析 {}: {e}", target.path))?;
                let before = value.clone();
                let changes = native_policy::json(&mut value, &disabled)
                    + memory_policy::apply(&mut value, shared_biome);
                for group in [
                    "models",
                    "skeletons",
                    "animations",
                    "textures",
                    "json",
                    "physics",
                ] {
                    let old = before
                        .get("dependencies")
                        .and_then(|d| d.get(group))
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len);
                    let new = value
                        .get("dependencies")
                        .and_then(|d| d.get(group))
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len);
                    if old > new {
                        *report
                            .counts
                            .entry(format!("removed_preload_{group}"))
                            .or_default() += old - new;
                    }
                }
                if shared_biome
                    && value.get("type").and_then(serde_json::Value::as_str) == Some("Preset")
                {
                    *report
                        .counts
                        .entry("shared_biome_presets".into())
                        .or_default() += 1;
                }

                if changes == 0 {
                    note(
                        &mut report,
                        &target.path,
                        "unchanged_native",
                        "引用和加载链无需改变，不新增覆盖".into(),
                    );
                    continue;
                }
                report.details.push(Outcome {
                    path: target.path.clone(),
                    action: "native_reference_pruning".into(),
                    reason: format!(
                        "独立规则处理 {changes} 项：场景/地形裁剪、共享简化 biome、无引用模型/骨骼预加载清理；保留活动状态机与所需动画"
                    ),
                });
                serde_json::to_vec(&value).map_err(|e| e.to_string())
            }
            Action::NativeTexture => assets::texture(
                &original,
                if request.texture_size == 0 {
                    4
                } else {
                    request.texture_size
                },
            ),
            Action::NativeSprite => Err("UI sprite 尺寸变更已停用".into()),
            _ => unreachable!(),
        };
        let generated = match generated {
            Ok(bytes) => bytes,
            Err(reason) => {
                return Err(format!(
                    "无法执行独立生成规则 {}: {reason}；未回退到完整原版图片",
                    target.path
                ));
            }
        };
        if category == "texture" && generated == original {
            note(
                &mut report,
                &target.path,
                "unchanged_native",
                "没有体积收益，使用原版".into(),
            );
            continue;
        }
        write(&mpq.join(&target.path), &generated)?;
        report.original_transformed_bytes += original.len() as u64;
        report.generated_asset_bytes += generated.len() as u64;
        *report.counts.entry(category.into()).or_default() += 1;
    }
    write(
        &mpq.join("modinfo.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({"name":final_name,"savepath":"../"}))
            .map_err(|e| e.to_string())?,
    )?;
    write(&mpq.join("data/global/dataversionbuild.txt"), &version)?;
    write(
        &staging.path.join("lightweight-manifest.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )?;
    let readme=format!("D2R 轻量资源测试版 {}\r\n启动参数：{}\r\n\r\n只从本机游戏生成，不包含 lowHD 成品素材。目标范围参考 lowHD；处理规则依据游戏原版制定，不复现作者配置或遮罩。\r\n纹理尺寸选项 {}（0=自动4），sprite 策略 {}（1=保留原版尺寸）。\r\n逐资源策略、颜色差异、缺失及无需覆盖的资源见 lightweight-manifest.json。未确认策略会阻止生成。\r\n本版不重写非空粒子、不修改 missiles.txt、不新增房间工具或声纹。\r\nVFX 纹理使用原版；粒子关闭模式及新增的空覆盖见清单。正常音频加工功能仍可单独用于该成品。\r\n未启动游戏验证。源游戏、原 MOD 和已有同名产物不会覆盖。\r\n",report.profile,report.launch_arguments,request.texture_size,request.sprite_scale);
    write(&staging.path.join("README.txt"), readme.as_bytes())?;
    if destination.exists() {
        return Err("输出名称在生成期间被占用，未覆盖；请重试".into());
    }
    fs::rename(&staging.path, &destination).map_err(|e| e.to_string())?;
    staging.committed = true;
    progress(total, total, "完成");
    Ok(report)
}

pub fn cli(args: &[std::ffi::OsString], import: bool) -> Result<(), String> {
    use std::io::Write;
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("轻量资源生成（实验版）\n  lightweight --game <游戏目录> [--profile main|filler|min]\n    [--texture-size 0|1|2|4|8|16|32] 默认4；0=自动4\n    [--sprite-scale 1] 非空 UI sprite 保持原版尺寸\n    [--effects preserve|off] 默认preserve；off关闭范围内粒子及其直接依赖\n    [--output <输出父目录>] [--name <MOD名称>]\n    [--asset-types empty,json,texture,sprite] 默认全部\n    [--recipe <自定义.json.gz>] [--json|--events]\n\n开发用范围导入（仅路径、空覆盖意图和资源类型，不提取内容）：\n  lightweight-import --source <参考.mpq目录> --game <游戏目录> --profile main --output <新配方.json.gz>\n\n运行生成只需要本机游戏与程序内置配方，无需原 lowHD 包。\n未确认的策略阻止生成；不重写粒子内部结构，不修改原游戏或启用 MOD。");
        return Ok(());
    }
    let mut options = BTreeMap::new();
    let mut json = false;
    let mut events = false;
    let mut i = 0;
    while i < args.len() {
        let key = args[i].to_str().ok_or("选项名必须为 ASCII")?;
        if key == "--json" {
            json = true;
            i += 1;
            continue;
        }
        if key == "--events" {
            events = true;
            i += 1;
            continue;
        }
        let allowed = if import {
            vec!["--source", "--game", "--profile", "--output"]
        } else {
            vec![
                "--game",
                "--profile",
                "--output",
                "--name",
                "--texture-size",
                "--sprite-scale",
                "--effects",
                "--recipe",
                "--asset-types",
            ]
        };
        if !allowed.contains(&key) {
            return Err(format!("未知轻量化选项: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 缺少值"))?
            .clone();
        if options.insert(key.to_string(), value).is_some() {
            return Err(format!("重复选项: {key}"));
        }
        i += 2;
    }
    if json && events {
        return Err("--json 与 --events 不能同时使用".into());
    }
    let profile = options
        .get("--profile")
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "main".into());
    if !["main", "filler", "min"].contains(&profile.as_str()) {
        return Err("profile 仅支持 main、filler、min".into());
    }
    if import {
        let source = PathBuf::from(options.get("--source").ok_or("导入必须提供 --source")?);
        let output = PathBuf::from(
            options
                .get("--output")
                .ok_or("导入必须提供 --output 配方文件路径")?,
        );
        let game = PathBuf::from(
            options
                .get("--game")
                .ok_or("范围导入需要提供 --game 作为游戏目录标识")?,
        );
        let count = recipe::import(&source, &game, &profile, &output)?;
        println!(
            "已导入 {count} 个目标：{}（未包含源素材数据）",
            output.display()
        );
        return Ok(());
    }
    let request = Request {
        game: PathBuf::from(options.get("--game").ok_or("必须提供 --game 游戏目录")?),
        output: options.get("--output").map(PathBuf::from),
        name: options
            .get("--name")
            .map(|s| s.to_string_lossy().into_owned()),
        profile,
        texture_size: options
            .get("--texture-size")
            .map(|s| s.to_string_lossy().parse())
            .transpose()
            .map_err(|_| "纹理尺寸不是整数")?
            .unwrap_or(4),
        sprite_scale: options
            .get("--sprite-scale")
            .map(|s| s.to_string_lossy().parse())
            .transpose()
            .map_err(|_| "sprite 参数不是整数")?
            .unwrap_or(1),
        disable_scoped_particles: match options
            .get("--effects")
            .map(|s| s.to_string_lossy())
            .as_deref()
            .unwrap_or("preserve")
        {
            "preserve" => false,
            "off" => true,
            _ => return Err("--effects 使用 preserve 或 off".into()),
        },
        recipe_file: options.get("--recipe").map(PathBuf::from),
        asset_types: options
            .get("--asset-types")
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "empty,json,texture,sprite".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect(),
    };
    let result = build(request, |current, total, path| {
        if events {
            println!(
                "{}",
                serde_json::json!({"type":"progress","phase":"lightweight","percent":current*100/total.max(1),"message":path})
            );
            let _ = std::io::stdout().flush();
        } else if !json {
            eprintln!("轻量化 {current}/{total}: {path}");
        }
    });
    match result {
        Ok(report) => {
            if events {
                println!(
                    "{}",
                    serde_json::json!({"type":"completed","report":report})
                );
            } else if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else {
                println!("完成：{}\n启动参数：{}\n处理统计：{:?}\n资源内容：{} 字节。运行效果待游戏内确认。",report.mod_directory,report.launch_arguments,report.counts,report.generated_asset_bytes);
            }
            Ok(())
        }
        Err(error) => {
            if events {
                println!("{}", serde_json::json!({"type":"error","message":error}));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_ui_scale_is_rejected_before_any_game_or_output_access() {
        let request = Request {
            game: PathBuf::from("nonexistent-game"),
            output: None,
            name: None,
            profile: "main".into(),
            texture_size: 4,
            sprite_scale: 2,
            disable_scoped_particles: false,
            recipe_file: None,
            asset_types: vec![
                "empty".into(),
                "json".into(),
                "texture".into(),
                "sprite".into(),
            ],
        };
        assert!(build(request, |_, _, _| {})
            .err()
            .unwrap()
            .contains("UI 布局未同步缩放"));
    }
    #[test]
    fn bundled_profiles_have_unique_safe_targets_and_no_asset_payloads() {
        for name in ["min", "filler", "main"] {
            let r = builtin(name).unwrap();
            assert_eq!(r.profile, name);
            assert_eq!(r.version, 4);
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::NativeJson)));

            assert!(r.targets.len() > 10000);
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::NativeSprite)));
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::NativeJson)));
            assert!(!r.targets.iter().any(|t| t.path == "modinfo.json"));
        }
    }
    #[test]
    fn failed_transaction_cleanup_does_not_touch_neighbors() {
        let parent = std::env::temp_dir().join(format!("lw-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        let parent = parent.canonicalize().unwrap();
        let keep = parent.join("keep.txt");
        fs::write(&keep, b"keep").unwrap();
        let path = parent.join(format!(".lightweight-building-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("partial.json"), b"{}").unwrap();
        {
            let _guard = Staging {
                path: path.clone(),
                parent: parent.clone(),
                committed: false,
            };
        }
        assert!(!path.exists());
        assert_eq!(fs::read(&keep).unwrap(), b"keep");
        fs::remove_file(keep).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
