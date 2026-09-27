//! Compatibility reconstruction with a byte-exact template mode for comparison.
mod assets;
mod bundled;
mod compatible;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
pub struct Request {
    pub game: PathBuf,
    pub source: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub name: Option<String>,
    pub profile: String,
    pub rebuild: bool,
}
pub fn default_name(profile: &str) -> &'static str {
    match profile {
        "main" => "LiteHub",
        "filler" => "BoHub",
        "min" => "NullHub",
        _ => "D2RNative",
    }
}
#[derive(Serialize)]
pub struct Report {
    pub producer: String,
    pub producer_version: String,
    pub mode: String,
    pub profile: String,
    pub source_directory: String,
    pub mod_name: String,
    pub mod_directory: String,
    pub launch_arguments: String,
    pub counts: BTreeMap<String, usize>,
    pub generated_bytes: u64,
    pub verified_compatible: bool,
    pub verified_output_integrity: bool,
    pub origins: Vec<compatible::Origin>,
    pub verified_identical_except_modinfo: bool,
    pub runtime_verified: bool,
    pub game_data_version: Option<String>,
    pub verified_b12_except_name_and_data_version: bool,
}
fn linked(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}
fn list(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        let m = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if linked(&m) {
            return Err(format!("模板不接受链接/重解析点：{}", p.display()));
        }
        if m.is_dir() {
            list(root, &p, out)?;
        } else if m.is_file() {
            out.push(
                p.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}
fn valid_name(n: &str) -> bool {
    let u = n.to_ascii_uppercase();
    !n.is_empty()
        && n.len() <= 64
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(u.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(u.len() == 4
            && (u.starts_with("COM") || u.starts_with("LPT"))
            && matches!(u.as_bytes()[3], b'1'..=b'9'))
}
fn parse(b: &[u8]) -> Result<Value, String> {
    let s = std::str::from_utf8(b)
        .map_err(|e| e.to_string())?
        .trim_start_matches('\u{feff}');
    json5::from_str(s).map_err(|e| e.to_string())
}
/// Replace only name's quoted value, preserving source comments and attribution.
fn rename_info(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut expected = parse(bytes)?;
    if !expected.is_object() || !expected.get("name").is_some_and(Value::is_string) {
        return Err("modinfo 缺少字符串 name".into());
    }
    expected["name"] = Value::String(name.into());
    let mut text = String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())?;
    let key = text
        .find("\"name\"")
        .ok_or("modinfo 的 name 必须带双引号")?
        + 6;
    let colon = key + text[key..].find(':').ok_or("name 缺少冒号")?;
    let start = colon + 1 + text[colon + 1..].len() - text[colon + 1..].trim_start().len();
    if text.as_bytes().get(start) != Some(&b'"') {
        return Err("name 值必须带双引号".into());
    }
    let (mut end, mut escaped) = (start + 1, false);
    while end < text.len() {
        let c = text.as_bytes()[end];
        if c == b'"' && !escaped {
            break;
        }
        escaped = c == b'\\' && !escaped;
        end += 1;
    }
    if end == text.len() {
        return Err("name 字符串未闭合".into());
    }
    text.replace_range(start..end + 1, &serde_json::to_string(name).unwrap());
    if parse(text.as_bytes())? != expected {
        return Err("不能仅修改名称，未发布".into());
    }
    Ok(text.into_bytes())
}
struct Stage {
    path: PathBuf,
    parent: PathBuf,
    committed: bool,
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.committed {
            if let Ok(p) = self.path.canonicalize() {
                if p.parent() == Some(self.parent.as_path())
                    && p.file_name()
                        .is_some_and(|s| s.to_string_lossy().starts_with(".hub-building-"))
                {
                    let _ = fs::remove_dir_all(p);
                }
            }
        }
    }
}
fn write(p: &Path, b: &[u8]) -> Result<(), String> {
    fs::create_dir_all(p.parent().ok_or("无父目录")?).map_err(|e| e.to_string())?;
    fs::write(p, b).map_err(|e| format!("写入 {}：{e}", p.display()))
}
// In-process write/read integrity check, not a security or provenance hash.
fn fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}
fn template(r: &Request) -> Result<PathBuf, String> {
    if let Some(p) = &r.source {
        return Ok(p.clone());
    }
    let n = format!("lowHD{}", r.profile);
    let outer = r.game.join("mods").join(&n);
    for p in [
        outer.join(format!("{n}.mpq")),
        outer.join(&n).join(format!("{n}.mpq")),
    ] {
        if p.join("modinfo.json").is_file() {
            return Ok(p);
        }
    }
    Err(format!("找不到本机 {n} 原包。请放入游戏 mods，或用 --source 指定含 modinfo.json 的模板目录。本模式不会从 CASC 猜测复刻。"))
}
pub fn build(r: Request, mut progress: impl FnMut(usize, usize, &str)) -> Result<Report, String> {
    if !["main", "filler", "min"].contains(&r.profile.as_str()) {
        return Err("profile 使用 main、filler 或 min".into());
    }
    if r.rebuild {
        return bundled::build(r, progress);
    }
    let source = template(&r)?.canonicalize().map_err(|e| e.to_string())?;
    let game_alias = if r.rebuild {
        Some(crate::casc_path::CascStoragePath::prepare(&r.game)?)
    } else {
        None
    };
    let storage = game_alias
        .as_ref()
        .map(|a| casc_core::Storage::open(a.as_path()).map_err(|e| e.to_string()))
        .transpose()?;
    let info = fs::read(source.join("modinfo.json")).map_err(|e| e.to_string())?;
    let base = r.name.unwrap_or_else(|| {
        format!(
            "{}-{}",
            if r.rebuild { "D2RNative" } else { "D2RLowHD" },
            r.profile
        )
    });
    if !valid_name(&base) {
        return Err("MOD 名称必须是安全的 ASCII 字母、数字、- 或 _".into());
    }
    let parent = r.output.unwrap_or_else(|| r.game.join("mods"));
    let game_data = r.game.join("Data").canonicalize().ok();
    // Check existing ancestors before create_dir_all so a rejected destination
    // cannot even create an empty directory inside the original template/data.
    for ancestor in parent.ancestors() {
        if let Ok(actual) = ancestor.canonicalize() {
            if actual.starts_with(&source)
                || game_data
                    .as_ref()
                    .is_some_and(|data| actual.starts_with(data))
            {
                return Err("输出目录不能位于模板或游戏 Data 内部".into());
            }
        }
    }
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&source) {
        return Err("输出目录不能位于模板内部".into());
    }
    if let Ok(data) = r.game.join("Data").canonicalize() {
        if parent.starts_with(data) {
            return Err("不能写入游戏 Data".into());
        }
    }
    let (mut name, mut suffix) = (base.clone(), 2);
    while parent.join(&name).exists() {
        name = format!("{base}-{suffix}");
        suffix += 1;
    }
    let renamed = rename_info(&info, &name)?;
    let mut paths = Vec::new();
    list(&source, &source, &mut paths)?;
    paths.sort();
    let total = paths.len();
    let destination = parent.join(&name);
    let mut stage = Stage {
        path: parent.join(format!(".hub-building-{}", uuid::Uuid::new_v4())),
        parent: parent.clone(),
        committed: false,
    };
    fs::create_dir(&stage.path).map_err(|e| e.to_string())?;
    let mpq = stage.path.join(format!("{name}.mpq"));
    let (mut counts, mut copied_bytes) = (BTreeMap::new(), 0);
    let mut semantic_paths = std::collections::HashSet::new();
    let mut origins = Vec::new();
    let mut alternatives = BTreeMap::new();
    let mut omitted = BTreeMap::new();
    for (i, rel) in paths.iter().enumerate() {
        if i % 200 == 0 {
            progress(
                i,
                total * 2,
                if r.rebuild {
                    "游戏重建及兼容资源处理"
                } else {
                    "复制原模板"
                },
            );
        }
        let mut bytes = if rel == Path::new("modinfo.json") {
            renamed.clone()
        } else {
            fs::read(source.join(rel)).map_err(|e| format!("读取 {}：{e}", rel.display()))?
        };
        if rel != Path::new("modinfo.json") {
            let path = rel.to_string_lossy().replace('\\', "/");
            if let Some(storage) = &storage {
                let generated = compatible::generate(storage, &path, &bytes)?;
                if generated.semantic_json {
                    semantic_paths.insert(rel.clone());
                }
                if generated.native_alternative {
                    alternatives.insert(
                        rel.clone(),
                        (fingerprint(&bytes), fingerprint(&generated.bytes)),
                    );
                }
                *counts.entry(generated.method.to_string()).or_default() += 1;
                origins.push(compatible::Origin {
                    path,
                    method: generated.method.into(),
                    game_source: generated.game_source,
                    equality: if generated.omit_file {
                        "not_emitted_missing_in_native_game"
                    } else if generated.semantic_json {
                        "parsed_json_equal_except_removed_auto_exit"
                    } else if generated.native_alternative {
                        "native_alternative_not_reference_equal"
                    } else {
                        "byte_equal"
                    }
                    .into(),
                });
                if generated.omit_file {
                    omitted.insert(rel.clone(), fingerprint(&bytes));
                    continue;
                }
                bytes = generated.bytes;
            }
        }
        write(&mpq.join(rel), &bytes)?;
        copied_bytes += bytes.len() as u64;
        *counts
            .entry(if bytes.is_empty() {
                "empty_files".into()
            } else {
                rel.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("other")
                    .to_string()
            })
            .or_default() += 1;
    }
    let mut current = Vec::new();
    list(&source, &source, &mut current)?;
    current.sort();
    if current != paths {
        return Err("模板清单在复制期间变化".into());
    }
    let mut all_bytes_equal = true;
    for (i, rel) in paths.iter().enumerate() {
        if i % 200 == 0 {
            progress(total + i, total * 2, "逐文件验证配置和资源一致性");
        }
        let expected = if rel == Path::new("modinfo.json") {
            if fs::read(source.join(rel)).map_err(|e| e.to_string())? != info {
                return Err("模板 modinfo 在复制期间变化".into());
            }
            renamed.clone()
        } else {
            fs::read(source.join(rel)).map_err(|e| e.to_string())?
        };
        if let Some(source_hash) = omitted.get(rel) {
            if fingerprint(&expected) != *source_hash || mpq.join(rel).exists() {
                return Err(format!("遗漏项校验失败：{}", rel.display()));
            }
            all_bytes_equal = false;
            continue;
        }
        let actual = fs::read(mpq.join(rel)).map_err(|e| e.to_string())?;
        if let Some((source_hash, generated_hash)) = alternatives.get(rel) {
            if fingerprint(&expected) != *source_hash || fingerprint(&actual) != *generated_hash {
                return Err(format!("生成期间输入变化或输出损坏：{}", rel.display()));
            }
            all_bytes_equal &= actual == expected;
            continue;
        }
        if actual != expected {
            all_bytes_equal = false;
            if !semantic_paths.contains(rel)
                || !compatible::equal_json(
                    &rel.to_string_lossy().replace('\\', "/"),
                    &actual,
                    &expected,
                )
            {
                return Err(format!("配置/资源验证不一致：{}", rel.display()));
            }
        }
    }
    let mut actual = Vec::new();
    list(&mpq, &mpq, &mut actual)?;
    actual.sort();
    let output_paths: Vec<_> = paths
        .iter()
        .filter(|p| !omitted.contains_key(*p))
        .cloned()
        .collect();
    if actual != output_paths {
        return Err("输出文件集不一致".into());
    }
    counts.insert("verified_files".into(), output_paths.len());
    counts.insert("omitted_missing_native_assets".into(), omitted.len());
    let txt = source.join("data/global/excel/missiles.txt").is_file();
    let report = Report {
        producer: "d2r-lowhd-compatible-generator".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        mode: if r.rebuild {
            "compatible_rebuild"
        } else {
            "exact_local_template"
        }
        .into(),
        profile: r.profile,
        source_directory: source.to_string_lossy().into_owned(),
        mod_name: name.clone(),
        mod_directory: destination.to_string_lossy().into_owned(),
        launch_arguments: format!("-mod {name}{}", if txt { " -txt" } else { "" }),
        counts,
        generated_bytes: copied_bytes,
        verified_compatible: !r.rebuild,
        verified_output_integrity: true,
        origins,
        verified_identical_except_modinfo: all_bytes_equal,
        runtime_verified: false,
        game_data_version: None,
        verified_b12_except_name_and_data_version: false,
    };
    write(
        &stage.path.join("generation-manifest.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )?;
    let notes = if r.rebuild {
        "sprite 恢复 b10 的低清资源选取与帧处理，撤回 b11 强制换回高清图集的规则。光标和小地图的显示比例尚未单独修复。非空图片及其他二进制仅来自游戏；不读取参考图片的像素遮罩，不复制参考成品。\r\nJSON/frontend 按参考配置重建并核验，四份暂停布局移除自动退出计时器，保留手动退出按钮；其他文本配置注明来源。不能简化的二进制保留原版并在清单标注 game_original_unsimplified；原版缺失的纹理及实验 DDS 不输出并逐项记录；其他未知资源缺失则停止生成。\r\n当前仍需本机 lowHD 提供目标清单、配置与尺寸，不等于生成器已完全脱离参考包。\r\n已核验文件集与写入完整性；原版替代图片可能不同于参考，不能以此宣称游戏效果和内存一致。"
    } else {
        "本模式直接复制本机参考包用于对照，除 modinfo 名称外逐字节一致。"
    };
    write(&stage.path.join("README.txt"), format!("lowHD 策略参考生成版 {}\r\n启动：{}\r\n参考：{}\r\n优化策略及配置参考 lowHD（celloboy126 / evilbelgian）。生成器实现为本项目代码，不将参考配置宣称为原创。\r\n模式：{}。各文件来源见 generation-manifest.json 的 origins。\r\n{}\r\n未启动游戏，实际画面及内存待实测。main 的 -txt 用于参考导弹表。\r\n", report.profile, report.launch_arguments, report.source_directory, report.mode, notes).as_bytes())?;
    if destination.exists() {
        return Err("输出名称被占用".into());
    }
    fs::rename(&stage.path, &destination).map_err(|e| e.to_string())?;
    stage.committed = true;
    progress(total * 2, total * 2, "完成");
    Ok(report)
}
pub fn cli(args: &[std::ffi::OsString], import: bool) -> Result<(), String> {
    if import {
        return Err("无需导入规则；请用 lightweight --game <目录> --profile main|filler|min。仅历史对照 --mode template 接受 --source".into());
    }
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("独立生成（游戏数据版本跟随原版，其余行为保持 b12）\nlightweight --game <游戏目录> --profile main|filler|min\n [--output <父目录>] [--name <名称>] [--json|--events]\n默认名称：main=LiteHub，filler=BoHub，min=NullHub。\n默认 --mode rebuild：读取内置规则和 CASC，逐文件核验 b12 校验值；不接受 --source。\n历史原包复制对照：--mode template [--source <模板目录>]，仅此模式需要外部原包。");
        return Ok(());
    }
    let (mut opts, mut json, mut events, mut i) = (BTreeMap::new(), false, false, 0);
    while i < args.len() {
        let k = args[i].to_str().ok_or("非法选项")?;
        if k == "--json" {
            json = true;
            i += 1;
            continue;
        }
        if k == "--events" {
            events = true;
            i += 1;
            continue;
        }
        if ![
            "--game",
            "--source",
            "--profile",
            "--output",
            "--name",
            "--mode",
        ]
        .contains(&k)
        {
            return Err(format!(
                "未知或停用选项 {k}；兼容模式不接受未经验证的缩图参数"
            ));
        }
        let v = args
            .get(i + 1)
            .ok_or_else(|| format!("{k} 缺少值"))?
            .clone();
        if opts.insert(k.to_string(), v).is_some() {
            return Err(format!("重复选项 {k}"));
        }
        i += 2;
    }
    if json && events {
        return Err("--json 与 --events 不能同时使用".into());
    }
    let r = Request {
        game: PathBuf::from(opts.get("--game").ok_or("必须提供 --game")?),
        source: opts.get("--source").map(PathBuf::from),
        output: opts.get("--output").map(PathBuf::from),
        name: opts.get("--name").map(|s| s.to_string_lossy().into_owned()),
        rebuild: match opts
            .get("--mode")
            .map(|s| s.to_string_lossy())
            .as_deref()
            .unwrap_or("rebuild")
        {
            "rebuild" => true,
            "template" => false,
            _ => return Err("--mode 使用 rebuild 或 template".into()),
        },
        profile: opts
            .get("--profile")
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".into()),
    };
    let result = build(r, |n, t, msg| {
        if events {
            println!(
                "{}",
                serde_json::json!({"type":"progress","phase":"lightweight","percent":n*100/t.max(1),"message":msg})
            );
            let _ = std::io::stdout().flush();
        } else if !json {
            eprintln!("{n}/{t} {msg}");
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
                println!(
                    "完成：{}\n启动：{}\n已验证输出完整性；原版算法替代资源的显示与内存效果待游戏实测。",
                    report.mod_directory, report.launch_arguments
                );
            }
            Ok(())
        }
        Err(e) => {
            if events {
                println!("{}", serde_json::json!({"type":"error","message":e}));
            }
            Err(e)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_attribution_and_savepath() {
        let b = b"{\n \"name\": \"lowHDmin\", \"savepath\": \"../\"\n}\n// by original author\n";
        let out = rename_info(b, "D2RLowHD-min").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            String::from_utf8(b.to_vec())
                .unwrap()
                .replace("lowHDmin", "D2RLowHD-min")
        );
    }
    #[test]
    fn copies_empty_binary_json5_and_original_version() {
        let root = std::env::temp_dir().join(format!("lowhd-test-{}", uuid::Uuid::new_v4()));
        let src = root.join("source");
        write(
            &src.join("modinfo.json"),
            b"{\"name\":\"original\",\"savepath\":\"../\"}\n// author",
        )
        .unwrap();
        for (p, b) in [
            ("data/empty.json", &b""[..]),
            ("data/custom.json", &b"{value:25,name:'custom'}"[..]),
            ("data/image.sprite", &[0, 255, 17][..]),
            ("data/global/dataversionbuild.txt", &b"old-build"[..]),
        ] {
            write(&src.join(p), b).unwrap();
        }
        let r = build(
            Request {
                game: root.clone(),
                source: Some(src.clone()),
                output: Some(root.join("out")),
                name: Some("test-mod".into()),
                profile: "min".into(),
                rebuild: false,
            },
            |_, _, _| {},
        )
        .unwrap();
        assert!(r.verified_identical_except_modinfo);
        assert_eq!(r.counts["verified_files"], 5);
        for p in [
            "data/empty.json",
            "data/custom.json",
            "data/image.sprite",
            "data/global/dataversionbuild.txt",
        ] {
            assert_eq!(
                fs::read(src.join(p)).unwrap(),
                fs::read(Path::new(&r.mod_directory).join("test-mod.mpq").join(p)).unwrap()
            );
        }
        assert!(build(
            Request {
                game: root.clone(),
                source: Some(src.clone()),
                output: Some(src.join("bad")),
                name: None,
                profile: "min".into(),
                rebuild: false,
            },
            |_, _, _| {}
        )
        .is_err());
        assert!(!src.join("bad").exists());
        let actual = root.canonicalize().unwrap();
        assert_eq!(
            actual.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(actual
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("lowhd-test-"));
        fs::remove_dir_all(actual).unwrap();
    }
}
