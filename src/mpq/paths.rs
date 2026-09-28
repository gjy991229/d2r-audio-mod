use super::{error, Result};
use std::collections::HashSet;
pub(super) fn validate_relative(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 4096 || path.contains('\\') {
        return Err(error("UNSAFE_PATH", "无效的资源相对路径"));
    }
    for part in path.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            )
            || ["COM", "LPT"].iter().any(|p| {
                stem.strip_prefix(p).is_some_and(|s| {
                    matches!(
                        s,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            })
        {
            return Err(error("UNSAFE_PATH", format!("不安全的资源路径：{path}")));
        }
    }
    Ok(())
}
pub(super) fn archive_name(raw: &[u8]) -> Result<(String, bool)> {
    let raw = raw
        .iter()
        .map(|b| if *b == b'\\' { b'/' } else { *b })
        .collect::<Vec<_>>();
    let mut escaped = false;
    let parts = raw
        .split(|b| *b == b'/')
        .map(|part| {
            if let Ok(s) = std::str::from_utf8(part) {
                s.to_owned()
            } else {
                escaped = true;
                part.iter()
                    .map(|b| {
                        if b.is_ascii() && *b != b'%' {
                            (*b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect()
            }
        })
        .collect::<Vec<_>>();
    let path = parts.join("/");
    validate_relative(&path)?;
    let lower = path.to_ascii_lowercase();
    if lower.len() == 16
        && lower.starts_with("file")
        && lower.ends_with(".xxx")
        && lower[4..12].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(error(
            "UNRESOLVED_ENTRIES",
            "MPQ 包含无法恢复原始文件名的条目",
        ));
    }
    Ok((path, escaped))
}
pub(super) fn validate_collisions<'a>(paths: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut files = HashSet::new();
    let mut dirs = HashSet::new();
    for path in paths {
        validate_relative(path)?;
        let key = path.to_lowercase();
        if dirs.contains(&key) || !files.insert(key.clone()) {
            return Err(error("PATH_CONFLICT", format!("重复或冲突的路径：{path}")));
        }
        let mut current = key.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            if files.contains(parent) {
                return Err(error("PATH_CONFLICT", format!("文件与目录冲突：{path}")));
            }
            dirs.insert(parent.to_string());
            current = parent;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_and_colliding_paths() {
        for path in [
            "../x",
            "/x",
            "C:/x",
            "data/x:ads",
            "data/CON.txt",
            "data/x.",
            "data//x",
            "data/LPT¹.txt",
        ] {
            assert!(validate_relative(path).is_err(), "{path}");
        }
        for paths in [
            vec!["data/A", "data/a"],
            vec!["data/a/x", "data/a"],
            vec!["data/a", "data/a/x"],
        ] {
            assert!(validate_collisions(paths.into_iter()).is_err());
        }
    }
    #[test]
    fn encoding_is_deterministic_and_unknown_names_fail() {
        assert_eq!(
            archive_name(b"data\\test.json").unwrap(),
            ("data/test.json".into(), false)
        );
        assert_eq!(
            archive_name(b"data/x-\xce\xde\xbf\xec\xbd\xa8\xb7\xbf.json").unwrap(),
            ("data/x-%CE%DE%BF%EC%BD%A8%B7%BF.json".into(), true)
        );
        assert!(archive_name(b"File00000001.xxx").is_err());
    }
}
