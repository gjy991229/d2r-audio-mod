use std::path::{Path, PathBuf};

/// Keeps an ASCII-only alias alive while CascLib may still lazily open files from the storage.
///
/// The pinned CascLib build uses Windows ANSI filesystem APIs. Rust and the rest of the
/// generator are Unicode-safe, so only the storage root needs this compatibility boundary.
pub(crate) struct CascStoragePath {
    path: PathBuf,
    #[cfg(windows)]
    alias: Option<PathBuf>,
}

impl CascStoragePath {
    pub(crate) fn prepare(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        {
            if path.to_string_lossy().is_ascii() {
                return Ok(Self {
                    path: path.to_path_buf(),
                    alias: None,
                });
            }

            let roots = windows_alias_roots();
            Self::prepare_with_roots(path, roots)
        }

        #[cfg(not(windows))]
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.path
    }

    #[cfg(windows)]
    fn prepare_with_roots(
        path: &Path,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, String> {
        let target = std::fs::canonicalize(path)
            .map_err(|error| format!("解析 Unicode 游戏目录失败 {}: {error}", path.display()))?;
        let mut failures = Vec::new();

        for root in roots {
            if !root.to_string_lossy().is_ascii() {
                continue;
            }
            if let Err(error) = std::fs::create_dir_all(&root) {
                failures.push(format!("{}: {error}", root.display()));
                continue;
            }
            let canonical_root = match std::fs::canonicalize(&root) {
                Ok(value) if value.to_string_lossy().is_ascii() => value,
                Ok(value) => {
                    failures.push(format!("{}: 实际路径不是 ASCII", value.display()));
                    continue;
                }
                Err(error) => {
                    failures.push(format!("{}: {error}", root.display()));
                    continue;
                }
            };
            let alias = canonical_root.join(format!(
                "casc-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            match junction::create(&target, &alias) {
                Ok(()) => {
                    return Ok(Self {
                        path: alias.clone(),
                        alias: Some(alias),
                    })
                }
                Err(error) => {
                    let _ = std::fs::remove_dir(&alias);
                    failures.push(format!("{}: {error}", alias.display()));
                }
            }
        }

        Err(format!(
            "游戏目录包含非 ASCII 字符，但无法创建 CASC 兼容路径{}",
            if failures.is_empty() {
                String::new()
            } else {
                format!("：{}", failures.join("；"))
            }
        ))
    }
}

#[cfg(windows)]
impl Drop for CascStoragePath {
    fn drop(&mut self) {
        if let Some(alias) = self.alias.as_ref() {
            let _ = junction::delete(alias);
            let _ = std::fs::remove_dir(alias);
        }
    }
}

#[cfg(windows)]
fn windows_alias_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(std::env::temp_dir().join("d2r-audio-mod-casc"));
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.push(
            PathBuf::from(program_data)
                .join("D2RAudioMod")
                .join("casc-paths"),
        );
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push(
            PathBuf::from(public)
                .join("Documents")
                .join("D2RAudioMod")
                .join("casc-paths"),
        );
    }
    if let Some(system_drive) = std::env::var_os("SystemDrive") {
        let system_drive = system_drive.to_string_lossy();
        roots.push(
            PathBuf::from(format!(
                r"{}\ProgramData",
                system_drive.trim_end_matches(['\\', '/'])
            ))
            .join("D2RAudioMod")
            .join("casc-paths"),
        );
    }
    roots
}

#[cfg(all(test, windows))]
mod tests {
    use super::CascStoragePath;

    #[test]
    fn leaves_ascii_paths_with_shell_characters_unchanged() {
        let path = std::path::Path::new(r"C:\Games & Tools\D2R (CN)");
        let prepared = CascStoragePath::prepare(path).unwrap();
        assert_eq!(prepared.as_path(), path);
    }

    #[test]
    fn creates_and_removes_an_ascii_alias_for_a_unicode_path() {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("casc-path-test-{}", uuid::Uuid::new_v4().simple()));
        let target = root.join("游戏路径 & (测试)");
        let alias_root = root.join("ascii-aliases");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("marker.txt"), b"ok").unwrap();

        let prepared = CascStoragePath::prepare_with_roots(&target, [alias_root]).unwrap();
        let alias = prepared.as_path().to_path_buf();
        assert!(alias.to_string_lossy().is_ascii());
        assert_eq!(std::fs::read(alias.join("marker.txt")).unwrap(), b"ok");

        drop(prepared);
        assert!(!alias.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
