//! Exact local lowHD template generation. No asset reconstruction.
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
    pub copied_bytes: u64,
    pub verified_identical_except_modinfo: bool,
    pub runtime_verified: bool,
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
                        .is_some_and(|s| s.to_string_lossy().starts_with(".lowhd-copy-"))
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
    let source = template(&r)?.canonicalize().map_err(|e| e.to_string())?;
    let info = fs::read(source.join("modinfo.json")).map_err(|e| e.to_string())?;
    let base = r.name.unwrap_or_else(|| format!("D2RLowHD-{}", r.profile));
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
        path: parent.join(format!(".lowhd-copy-{}", uuid::Uuid::new_v4())),
        parent: parent.clone(),
        committed: false,
    };
    fs::create_dir(&stage.path).map_err(|e| e.to_string())?;
    let mpq = stage.path.join(format!("{name}.mpq"));
    let (mut counts, mut copied_bytes) = (BTreeMap::new(), 0);
    for (i, rel) in paths.iter().enumerate() {
        if i % 200 == 0 {
            progress(i, total * 2, "复制原模板");
        }
        let bytes = if rel == Path::new("modinfo.json") {
            renamed.clone()
        } else {
            fs::read(source.join(rel)).map_err(|e| format!("读取 {}：{e}", rel.display()))?
        };
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
    for (i, rel) in paths.iter().enumerate() {
        if i % 200 == 0 {
            progress(total + i, total * 2, "逐文件验证字节一致");
        }
        let expected = if rel == Path::new("modinfo.json") {
            if fs::read(source.join(rel)).map_err(|e| e.to_string())? != info {
                return Err("模板 modinfo 在复制期间变化".into());
            }
            renamed.clone()
        } else {
            fs::read(source.join(rel)).map_err(|e| e.to_string())?
        };
        if fs::read(mpq.join(rel)).map_err(|e| e.to_string())? != expected {
            return Err(format!("文件字节不一致：{}", rel.display()));
        }
    }
    let mut actual = Vec::new();
    list(&mpq, &mpq, &mut actual)?;
    actual.sort();
    if actual != paths {
        return Err("输出文件集不一致".into());
    }
    counts.insert("verified_files".into(), total);
    let txt = source.join("data/global/excel/missiles.txt").is_file();
    let report = Report {
        producer: "d2r-lowhd-template-generator".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        mode: "exact_local_template".into(),
        profile: r.profile,
        source_directory: source.to_string_lossy().into_owned(),
        mod_name: name.clone(),
        mod_directory: destination.to_string_lossy().into_owned(),
        launch_arguments: format!("-mod {name}{}", if txt { " -txt" } else { "" }),
        counts,
        copied_bytes,
        verified_identical_except_modinfo: true,
        runtime_verified: false,
    };
    write(
        &stage.path.join("generation-manifest.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )?;
    write(&stage.path.join("README.txt"),format!("本机 lowHD 模板生成版 {}\r\n启动：{}\r\n模板：{}\r\n全部文件直接复制，仅修改 modinfo.json 的 name，保留原作者注释、savepath 和原数据版本标记。\r\n已逐文件验证字节一致，不套用独立精简、缩图或粒子关闭规则。\r\n本模式需要本机 lowHD 原包，不是从游戏 CASC 独立重建。原内容不宣称为本工具原创，模板许可不因复制改变。\r\nmain 的 -txt 用于原包 missiles.txt 表格编译。未启动游戏验证。\r\n",report.profile,report.launch_arguments,report.source_directory).as_bytes())?;
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
        return Err("旧规则导入已停用；请用 lightweight --game <目录> --profile main|filler|min，或 --source 指定原包模板".into());
    }
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("按本机 lowHD 原包生成（不从 CASC 复刻）\nlightweight --game <游戏目录> --profile main|filler|min\n [--source <含 modinfo.json 的原包目录>] [--output <父目录>] [--name <名称>]\n [--json|--events]\n自动查找游戏 mods 中对应 lowHD 原包，仅改 MOD 名称。旧缩图、粒子、配方参数停用。");
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
        if !["--game", "--source", "--profile", "--output", "--name"].contains(&k) {
            return Err(format!("未知或停用选项 {k}；模板模式不改资源"));
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
        profile: opts
            .get("--profile")
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".into()),
    };
    let result = build(r, |n, t, msg| {
        if events {
            println!(
                "{}",
                serde_json::json!({"type":"progress","phase":"template","percent":n*100/t.max(1),"message":msg})
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
                    "完成：{}\n启动：{}\n除 modinfo 名称外全部与原包一致。",
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
                profile: "min".into()
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
