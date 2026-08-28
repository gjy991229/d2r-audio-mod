mod audio;
mod casc_path;
mod generator;
#[cfg(target_os = "windows")]
mod gui;

use audio::BatchRequest;
use d2r_audio_protocol::catalog::AREA_CATALOG_FILE_NAME;
use d2r_audio_protocol::item_catalog::{
    default_tracked_categories, normalize_tracked_categories, ITEM_CATALOG_FILE_NAME,
    SUPPORTED_TRACKING_CATEGORIES,
};
use d2r_audio_protocol::protocol::PROTOCOL_VERSION;
use generator::{AudioAreaCoverage, AudioModBuildMode, BuildAudioModRequest};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;

#[derive(Default)]
struct Options {
    game: Option<PathBuf>,
    source: Option<PathBuf>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    name: Option<String>,
    sound_environment: Option<PathBuf>,
    areas: Option<String>,
    track: Option<String>,
    gain_db: Option<f32>,
    json: bool,
    events: bool,
}

fn value_after(args: &[OsString], index: &mut usize, option: &str) -> Result<OsString, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} 后缺少参数"))
}

fn parse_options(args: &[OsString]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0usize;
    while index < args.len() {
        let key = args[index]
            .to_str()
            .ok_or_else(|| "选项名称必须是 ASCII 文本".to_string())?;
        match key {
            "--game" => options.game = Some(value_after(args, &mut index, key)?.into()),
            "--source" => options.source = Some(value_after(args, &mut index, key)?.into()),
            "--input" => options.input = Some(value_after(args, &mut index, key)?.into()),
            "--output" => options.output = Some(value_after(args, &mut index, key)?.into()),
            "--name" => {
                options.name = Some(
                    value_after(args, &mut index, key)?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--sound-environment" => {
                options.sound_environment = Some(value_after(args, &mut index, key)?.into())
            }
            "--areas" => {
                options.areas = Some(
                    value_after(args, &mut index, key)?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--track" => {
                options.track = Some(
                    value_after(args, &mut index, key)?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--gain" => {
                let value = value_after(args, &mut index, key)?;
                options.gain_db =
                    Some(value.to_string_lossy().parse::<f32>().map_err(|_| {
                        format!("--gain 不是有效数字: {}", value.to_string_lossy())
                    })?);
            }
            "--json" => options.json = true,
            "--events" => options.events = true,
            "-h" | "--help" => return Err("__help__".to_string()),
            _ => return Err(format!("未知选项: {key}")),
        }
        index += 1;
    }
    Ok(options)
}

fn path_text(path: Option<PathBuf>) -> Option<String> {
    path.map(|value| value.to_string_lossy().into_owned())
}

fn parse_area_coverage(raw: Option<&str>) -> Result<AudioAreaCoverage, String> {
    match raw.unwrap_or("all").trim().to_ascii_lowercase().as_str() {
        "all" => Ok(AudioAreaCoverage::AllAreas),
        "countess" => Ok(AudioAreaCoverage::CountessRoute),
        value => Err(format!("--areas 仅支持 all 或 countess，收到: {value}")),
    }
}

fn parse_tracking(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(raw) = raw else {
        return Ok(default_tracked_categories());
    };
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized == "all" {
        return Ok(default_tracked_categories());
    }
    if normalized == "none" {
        return Ok(Vec::new());
    }
    let requested = normalized
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let unknown = requested
        .iter()
        .filter(|category| {
            !SUPPORTED_TRACKING_CATEGORIES
                .iter()
                .any(|supported| supported == &category.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(format!(
            "--track 包含未知类别: {}。可用类别: {}",
            unknown.join(","),
            SUPPORTED_TRACKING_CATEGORIES.join(",")
        ));
    }
    Ok(normalize_tracked_categories(&requested))
}

fn run_build(mode: AudioModBuildMode, options: Options) -> Result<(), String> {
    if mode == AudioModBuildMode::Minimal && options.game.is_none() {
        return Err("minimal 模式必须提供 --game <D2R 游戏目录>".to_string());
    }
    if mode == AudioModBuildMode::Augment && options.source.is_none() {
        return Err("augment 模式必须提供 --source <源 Mod 或 .mpq 目录>".to_string());
    }
    if options.input.is_some() {
        return Err("--input 只用于 tag-files 命令".to_string());
    }
    if options.json && options.events {
        return Err("--json 与 --events 不能同时使用".to_string());
    }
    let request = BuildAudioModRequest {
        build_mode: mode,
        source_directory: path_text(options.source),
        game_directory: path_text(options.game),
        area_coverage: parse_area_coverage(options.areas.as_deref())?,
        tracked_categories: parse_tracking(options.track.as_deref())?,
        output_directory: path_text(options.output),
        mod_name: options.name,
        sound_environment_file: path_text(options.sound_environment),
        gain_db: options.gain_db,
    };
    let report = if options.events {
        match generator::build_with_progress(request, |progress| {
            let event = serde_json::json!({
                "type": "progress",
                "phase": progress.phase,
                "percent": progress.percent,
                "message": progress.message,
            });
            println!("{event}");
            let _ = std::io::stdout().flush();
        }) {
            Ok(report) => {
                let event = serde_json::json!({
                    "type": "completed",
                    "report": {
                        "protocol_version": report.protocol_version,
                        "mod_name": report.mod_name,
                        "mod_directory": report.mod_directory,
                        "launch_arguments": report.launch_arguments,
                    }
                });
                println!("{event}");
                let _ = std::io::stdout().flush();
                return Ok(());
            }
            Err(error) => {
                let event = serde_json::json!({ "type": "error", "message": error });
                println!("{event}");
                let _ = std::io::stdout().flush();
                return Err(error);
            }
        }
    } else {
        generator::build(request)?
    };
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("输出 JSON 失败: {error}"))?
        );
    } else {
        println!("完成：{}", report.mod_directory);
        println!("启动参数：{}", report.launch_arguments);
        println!(
            "协议 v{}；区域 {}；符文 {}；其他物品 {}；主界面资源 {}",
            report.protocol_version,
            report.area_assets.len(),
            report.rune_assets.len(),
            report.item_assets.len(),
            report.frontend_assets.len()
        );
        println!("工具没有修改源 Mod、账号配置或当前启用的 Mod。");
    }
    Ok(())
}

fn run_tag_files(options: Options) -> Result<(), String> {
    if options.game.is_some()
        || options.source.is_some()
        || options.sound_environment.is_some()
        || options.areas.is_some()
        || options.track.is_some()
        || options.name.is_some()
    {
        return Err("tag-files 仅接受 --input、--output、--gain 和 --json".to_string());
    }
    let input = options
        .input
        .ok_or_else(|| "tag-files 必须提供 --input <FLAC 目录>".to_string())?;
    let report = audio::process_directory(BatchRequest {
        input_directory: input.to_string_lossy().into_owned(),
        output_directory: path_text(options.output),
        gain_db: options.gain_db,
    })?;
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("输出 JSON 失败: {error}"))?
        );
    } else {
        println!("完成：{}", report.output_directory);
        println!("已加工 {} 个 FLAC；源文件未覆盖。", report.processed.len());
    }
    Ok(())
}

fn print_protocol() {
    println!("D2R Audio Telemetry Protocol v{PROTOCOL_VERSION}");
    println!("地图清单：{AREA_CATALOG_FILE_NAME}");
    println!("物品清单：{ITEM_CATALOG_FILE_NAME}");
    println!("Mod 清单：audio-telemetry-manifest.json");
}

fn print_help() {
    println!(
        r#"D2R 音频遥测 Mod 工具（独立于任何接收软件）

用法：
  d2r-audio-mod                 打开轻量生成界面（Windows）
  d2r-audio-mod gui             打开轻量生成界面（Windows）
  d2r-audio-mod minimal --game <游戏目录> [选项]
  d2r-audio-mod augment --source <源 Mod/.mpq> [--game <游戏目录>] [选项]
  d2r-audio-mod tag-files --input <FLAC 目录> [--output <目录>] [--gain -30]
  d2r-audio-mod protocol

Mod 选项：
  --output <目录>             输出父目录；省略时优先使用游戏的 mods 目录
  --name <名称>              自定义 Mod 名；仅允许 ASCII 字母、数字、- 和 _
  --areas all|countess       地图覆盖，默认 all
  --track all|none|类别列表   默认 all；列表以英文逗号分隔
  --gain <dBFS>              声纹增益，范围 -42 到 -12，默认 -30
  --sound-environment <文件> 显式指定 soundenviron.txt
  --json                     将完整结果写到标准输出
  --events                   逐行输出进度/完成/错误 JSON 事件，供外部程序调用

类别：runes,gems,charms,jewels,keys,organs,essences

本工具只生成文件，不读取 D2RHub 配置/数据库，不切换 Mod，不修改启动参数。"#
    );
}

fn real_main() -> Result<(), String> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        #[cfg(target_os = "windows")]
        return gui::run(true);
        #[cfg(not(target_os = "windows"))]
        {
            print_help();
            return Ok(());
        }
    };
    let rest = args.collect::<Vec<_>>();
    match command.to_string_lossy().to_ascii_lowercase().as_str() {
        "gui" => {
            if !rest.is_empty() {
                return Err("gui 命令不接受其他参数".to_string());
            }
            #[cfg(target_os = "windows")]
            {
                gui::run(false)
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err("图形界面目前仅支持 Windows".to_string())
            }
        }
        "minimal" => match parse_options(&rest) {
            Ok(options) => run_build(AudioModBuildMode::Minimal, options),
            Err(error) if error == "__help__" => {
                print_help();
                Ok(())
            }
            Err(error) => Err(error),
        },
        "augment" => match parse_options(&rest) {
            Ok(options) => run_build(AudioModBuildMode::Augment, options),
            Err(error) if error == "__help__" => {
                print_help();
                Ok(())
            }
            Err(error) => Err(error),
        },
        "tag-files" => match parse_options(&rest) {
            Ok(options) => run_tag_files(options),
            Err(error) if error == "__help__" => {
                print_help();
                Ok(())
            }
            Err(error) => Err(error),
        },
        "protocol" => {
            if !rest.is_empty() {
                return Err("protocol 命令不接受其他参数".to_string());
            }
            print_protocol();
            Ok(())
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "--version" => {
            println!(
                "d2r-audio-mod {} (protocol v{PROTOCOL_VERSION})",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        _ if command == OsStr::new("") => {
            print_help();
            Ok(())
        }
        _ => Err(format!("未知命令: {}", command.to_string_lossy())),
    }
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("错误：{error}");
        eprintln!("运行 d2r-audio-mod help 查看用法。");
        std::process::exit(2);
    }
}
