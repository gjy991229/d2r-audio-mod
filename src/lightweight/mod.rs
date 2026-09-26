mod assets;
mod blocks;
mod json_delta;
pub mod recipe;
mod reference;

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
        return Err("纹理尺寸使用 0（参考尺寸），或显式指定 1、2、4、8、16、32".into());
    }
    if ![0, 1, 2, 4, 8].contains(&request.sprite_scale) {
        return Err("sprite 使用 0（参考尺寸）、1（原版）或 2、4、8（参考处理后缩小）".into());
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
    let unresolved: Vec<_> = recipe
        .targets
        .iter()
        .filter_map(|t| {
            if let Action::Unresolved(reason) = &t.action {
                Some(format!("{}: {reason}", t.path))
            } else {
                None
            }
        })
        .collect();
    if !unresolved.is_empty() {
        return Err(format!(
            "参考策略尚有 {} 项未确认，未生成 MOD，也未恢复原版整图：\n{}",
            unresolved.len(),
            unresolved
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
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
        counts: BTreeMap::new(),
        original_transformed_bytes: 0,
        generated_asset_bytes: 0,
        details: Vec::new(),
        runtime_verified: false,
    };
    let total = recipe.targets.len();
    for (index, target) in recipe.targets.iter().enumerate() {
        if index % 100 == 0 {
            progress(index, total, &target.path);
        }
        let category = match target.action {
            Action::Empty => "empty",
            Action::Json(_) | Action::JsonDelta(_) => "json",
            Action::Texture | Action::ReferenceTexture(_) => "texture",
            Action::Sprite | Action::ReferenceSprite(_) => "sprite",
            Action::Native(_) | Action::Unresolved(_) => "native",
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
        if request.sprite_scale == 1 && matches!(target.action, Action::ReferenceSprite(_)) {
            note(
                &mut report,
                &target.path,
                "user_native",
                "用户显式选择原版非空 sprite".into(),
            );
            continue;
        }
        let source_path = if let Action::ReferenceSprite(plan) = &target.action {
            plan.source.as_str()
        } else if let Action::JsonDelta(plan) = &target.action {
            plan.source.as_str()
        } else {
            target.path.as_str()
        };
        let original = match read(&storage, source_path) {
            Ok(bytes) => bytes,
            Err(e) if missing(&e) => {
                if matches!(
                    target.action,
                    Action::ReferenceSprite(_) | Action::ReferenceTexture(_) | Action::JsonDelta(_)
                ) {
                    return Err(format!(
                        "配方对应的原版资源已缺失 {source_path}，请重新核对配方：{e}"
                    ));
                }
                note(&mut report, &target.path, "missing_native", e.to_string());
                continue;
            }
            Err(e) => return Err(format!("游戏资源读取失败 {}: {e}", target.path)),
        };
        let generated = match &target.action {
            Action::Json(rule) => {
                let mut value = recipe::parse(&original)
                    .map_err(|e| format!("原版 JSON 无法解析 {}: {e}", target.path))?;
                let before = value.clone();
                rule.apply(&mut value);
                if value == before {
                    note(
                        &mut report,
                        &target.path,
                        "unchanged_native",
                        "裁剪后定义未变化，不输出冗余覆盖".into(),
                    );
                    continue;
                }
                // LoadScreenPanel has implicit render behaviour independent of
                // its child list. Use an empty native panel shell for this cut.
                if target.path.ends_with("/loadscreenpanelhd.json") {
                    value = serde_json::json!({"type":"TitleScreenHDPanel","name":"LoadScreenPanel","fields":{"fitToParent":true}});
                }
                serde_json::to_vec(&value).map_err(|e| e.to_string())
            }
            Action::JsonDelta(plan) => {
                let value = plan.apply(&original)?;
                if plan.source == target.path && value == recipe::parse(&original)? {
                    note(
                        &mut report,
                        &target.path,
                        "unchanged_native",
                        "JSON 参考逻辑与原版一致，无需覆盖".into(),
                    );
                    continue;
                }
                serde_json::to_vec(&value).map_err(|e| e.to_string())
            }
            Action::ReferenceTexture(plan) => {
                reference::texture(&original, plan, request.texture_size)
            }
            Action::ReferenceSprite(plan) => {
                let result = reference::sprite_scaled(&original, plan, request.sprite_scale);
                if result.is_ok() {
                    report.details.push(Outcome{path:target.path.clone(),action:plan.policy.clone(),reason:format!("源 {}；参考 {}×{}、{} 帧；擦除矩形 {}，黑色填充矩形 {}；去除参考文件无效尾部 {} 字节；颜色采用原版：{}",plan.source,plan.output.width,plan.output.height,plan.output.frames,plan.clear_rects.len(),plan.black_rects.len(),plan.discarded_reference_tail_bytes,plan.native_colors_retained)});
                    report.details.push(Outcome {
                        path: target.path.clone(),
                        action: "sprite_output".into(),
                        reason: format!(
                            "参考处理后缩小 {} 倍；输出 {} 字节",
                            request.sprite_scale.max(1),
                            result.as_ref().unwrap().len()
                        ),
                    });
                }
                result
            }
            Action::Texture | Action::Sprite => Err("旧统一缩放策略已停用".into()),
            _ => unreachable!(),
        };
        let generated = match generated {
            Ok(bytes) => bytes,
            Err(reason) => {
                return Err(format!(
                    "无法执行已确认的参考策略 {}: {reason}；未回退到完整原版图片",
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
    let readme=format!("D2R 轻量资源测试版 {}\r\n启动参数：{}\r\n\r\n只从本机游戏生成，不包含 lowHD 成品素材。清单和结构选择参考 lowHD；不是完全相同的复制版。\r\n纹理尺寸选项 {}（0=参考），sprite 策略 {}（0=参考，1=用户选择原版，2/4/8=参考处理后缩小）。\r\n逐资源策略、颜色差异、缺失及无需覆盖的资源见 lightweight-manifest.json。未确认策略会阻止生成。\r\n本版不重写非空粒子、不修改 missiles.txt、不新增房间工具或声纹。\r\n正常音频加工功能仍可单独用于该成品。\r\n未启动游戏验证。源游戏、原 MOD 和已有同名产物不会覆盖。\r\n",report.profile,report.launch_arguments,request.texture_size,request.sprite_scale);
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
        println!("轻量资源生成（实验版）\n  lightweight --game <游戏目录> [--profile main|filler|min]\n    [--texture-size 0|1|2|4|8|16|32] 默认4；0=参考尺寸\n    [--sprite-scale 0|1|2|4|8] 默认2=参考处理后缩小2倍；0=参考尺寸；1=原版\n    [--output <输出父目录>] [--name <MOD名称>]\n    [--asset-types empty,json,texture,sprite] 默认全部\n    [--recipe <自定义.json.gz>] [--json|--events]\n\n开发用清单导入（不复制成品素材）：\n  lightweight-import --source <参考.mpq目录> --game <游戏目录> --profile main --output <新配方.json.gz>\n\n运行生成只需要本机游戏与程序内置配方，无需原 lowHD 包。\n未确认的策略阻止生成；不重写粒子内部结构，不修改原游戏或启用 MOD。");
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
                .ok_or("参考策略导入必须提供 --game 以比对原版")?,
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
            .map_err(|_| "sprite 缩小倍数不是整数")?
            .unwrap_or(2),
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
    fn bundled_profiles_have_unique_safe_targets_and_no_asset_payloads() {
        for name in ["min", "filler", "main"] {
            let r = builtin(name).unwrap();
            assert_eq!(r.profile, name);
            assert_eq!(r.version, 3);
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::JsonDelta(_))));
            assert!(!r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::Unresolved(_))));
            assert!(r.targets.len() > 10000);
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::ReferenceSprite(_))));
            assert!(r
                .targets
                .iter()
                .any(|t| matches!(t.action, Action::Json(_))));
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
