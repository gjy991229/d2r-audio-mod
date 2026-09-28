use super::{
    archive::{self, Archive, Record},
    error, paths, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
const JOURNAL: &str = ".d2rhub-unpack.json";

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    schema_version: u32,
    transaction_id: String,
    mod_name: String,
    archive_leaf: String,
    archive_sha256: String,
    archive_bytes: u64,
    phase: String,
    manifest_sha256: Option<String>,
}
struct Layout {
    root: PathBuf,
    source: PathBuf,
    stage: PathBuf,
    backup_dir: PathBuf,
    backup: PathBuf,
}
fn layout(root: &Path, j: &Journal) -> Result<Layout> {
    if j.schema_version != 1
        || uuid::Uuid::parse_str(&j.transaction_id).is_err()
        || root.file_name().and_then(|s| s.to_str()) != Some(&j.mod_name)
        || !j
            .archive_leaf
            .eq_ignore_ascii_case(&format!("{}.mpq", j.mod_name))
    {
        return Err(error("RECOVERY_REQUIRED", "转换日志身份无效"));
    }
    paths::validate_relative(&j.archive_leaf)?;
    if j.archive_leaf.contains('/') {
        return Err(error("RECOVERY_REQUIRED", "转换日志文件名越界"));
    }
    let backup_dir = root.join("back").join(&j.transaction_id);
    Ok(Layout {
        root: root.into(),
        source: root.join(&j.archive_leaf),
        stage: root.join(format!(".d2rhub-unpack-{}", j.transaction_id)),
        backup: backup_dir.join(&j.archive_leaf),
        backup_dir,
    })
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if m.file_attributes() & 0x400 != 0 || m.file_type().is_symlink() {
                return Err(error(
                    "UNSAFE_PATH",
                    format!("拒绝链接或重解析点：{}", path.display()),
                ));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn check_chain(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        exists(p)?;
    }
    Ok(())
}
fn move_no_replace(from: &Path, to: &Path) -> Result<()> {
    check_chain(from)?;
    check_chain(to)?;
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>()
    };
    if unsafe {
        MoveFileExW(
            wide(from).as_ptr(),
            wide(to).as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn hash_file(path: &Path) -> Result<String> {
    check_chain(path)?;
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(archive::hex(&hash.finalize()))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    check_chain(path.parent().unwrap())?;
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    check_chain(path)?;
    let temp = path.with_file_name(format!(".mpq-journal-{}.tmp", uuid::Uuid::new_v4()));
    write_new(&temp, &serde_json::to_vec(value)?)?;
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>()
    };
    let result = unsafe {
        MoveFileExW(
            wide(&temp).as_ptr(),
            wide(path).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn persist(l: &Layout, j: &mut Journal, phase: &str) -> Result<()> {
    j.phase = phase.into();
    atomic_json(&l.root.join(JOURNAL), j)
}
fn list_tree(root: &Path) -> Result<Vec<PathBuf>> {
    if !exists(root)? {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(dir)? {
            let p = item?.path();
            exists(&p)?;
            if p.is_dir() {
                pending.push(p);
            } else if p.is_file() {
                files.push(p);
            } else {
                return Err(error("UNSAFE_PATH", "非普通资源节点"));
            }
        }
    }
    Ok(files)
}
fn validate_tree(root: &Path, records: &[Record]) -> Result<()> {
    paths::validate_collisions(records.iter().map(|r| r.path.as_str()))?;
    let actual = list_tree(root)?;
    let names = actual
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                .to_lowercase()
        })
        .collect::<HashSet<_>>();
    if names.len() != records.len() {
        return Err(error("RECOVERY_REQUIRED", "转换目录文件数与清单不符"));
    }
    for r in records {
        let path = root.join(&r.path);
        if !names.contains(&r.path.to_lowercase())
            || fs::metadata(&path)?.len() != r.size
            || hash_file(&path)? != r.sha256
        {
            return Err(error(
                "RECOVERY_REQUIRED",
                format!("转换资源校验失败：{}", r.path),
            ));
        }
    }
    validate_mod(root)
}
fn validate_mod(root: &Path) -> Result<()> {
    if !root.join("data").is_dir() || !root.join("modinfo.json").is_file() {
        return Err(error(
            "INVALID_MOD_LAYOUT",
            "需要根目录 modinfo.json 和 data 文件夹",
        ));
    }
    let bytes = fs::read(root.join("modinfo.json"))?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let info: Value = serde_json::from_slice(bytes)?;
    if !info.is_object() {
        return Err(error("INVALID_MOD_LAYOUT", "modinfo.json 必须为 JSON 对象"));
    }
    Ok(())
}
fn verify_original(path: &Path, j: &Journal) -> Result<()> {
    if !exists(path)?
        || !path.is_file()
        || fs::metadata(path)?.len() != j.archive_bytes
        || hash_file(path)? != j.archive_sha256
    {
        return Err(error(
            "RECOVERY_REQUIRED",
            "原 MPQ 身份发生变化；已保留所有文件，请检查事务目录",
        ));
    }
    Ok(())
}
fn cleanup_stage(l: &Layout) -> Result<()> {
    if exists(&l.stage)? {
        check_chain(&l.stage)?;
        let resolved = fs::canonicalize(&l.stage)?;
        if resolved.parent() != Some(l.root.as_path()) {
            return Err(error("UNSAFE_PATH", "临时目录不在当前 Mod 外层目录内"));
        }
        list_tree(&l.stage)?;
        fs::remove_dir_all(&l.stage)?;
    }
    Ok(())
}
fn finish(l: &Layout, j: &Journal) -> Result<()> {
    atomic_json(&l.backup_dir.join("conversion.json"), j)?;
    cleanup_stage(l)?;
    fs::remove_file(l.root.join(JOURNAL))?;
    Ok(())
}
fn display(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        s.trim_start_matches(r"\\?\").to_string()
    }
}
fn report(l: &Layout, j: &Journal, status: &str, records: &[Record]) -> Value {
    json!({"schema_version":1,"status":status,"mod_name":j.mod_name,"mod_directory":display(&l.root),
        "mpq_directory":display(&l.source),"backup_path":display(&l.backup),"transaction_id":j.transaction_id,
        "archive_sha256":j.archive_sha256,"file_count":records.len(),"uncompressed_bytes":records.iter().map(|r| r.size).sum::<u64>(),
        "escaped_file_names":records.iter().filter(|r| r.escaped_name).map(|r| json!({"path":r.path,"archive_name_hex":r.archive_name_hex})).collect::<Vec<_>>()})
}
fn read_manifest(l: &Layout, j: &Journal) -> Result<Vec<Record>> {
    let path = l.backup_dir.join("files.json");
    if j.manifest_sha256.as_deref() != Some(hash_file(&path)?.as_str()) {
        return Err(error("RECOVERY_REQUIRED", "转换资源清单摘要不符"));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn recover(root: &Path) -> Result<Value> {
    let pointer = root.join(JOURNAL);
    if !exists(&pointer)? {
        return Ok(json!({"schema_version":1,"status":"no_transaction"}));
    }
    let mut j: Journal = serde_json::from_slice(&fs::read(&pointer)?)?;
    if !matches!(
        j.phase.as_str(),
        "extracting"
            | "prepared"
            | "backup_pending"
            | "publish_pending"
            | "committed"
            | "rolled_back"
    ) {
        return Err(error("RECOVERY_REQUIRED", "无法识别的转换日志阶段"));
    }
    let l = layout(root, &j)?;
    check_chain(&l.backup)?;
    check_chain(&l.stage)?;
    if !l.backup_dir.is_dir() {
        return Err(error("RECOVERY_REQUIRED", "备份事务目录缺失"));
    }
    let source_exists = exists(&l.source)?;
    let backup_exists = exists(&l.backup)?;
    if j.phase == "committed" {
        if !source_exists || !l.source.is_dir() || !backup_exists {
            return Err(error("RECOVERY_REQUIRED", "已提交转换的源或备份缺失"));
        }
        verify_original(&l.backup, &j)?;
        let records = read_manifest(&l, &j)?;
        finish(&l, &j)?;
        return Ok(report(&l, &j, "committed", &records));
    }
    if source_exists && l.source.is_file() && !backup_exists {
        verify_original(&l.source, &j)?;
        persist(&l, &mut j, "rolled_back")?;
        finish(&l, &j)?;
        return Ok(
            json!({"schema_version":1,"status":"rolled_back","mod_directory":display(root)}),
        );
    }
    if !source_exists && backup_exists {
        verify_original(&l.backup, &j)?;
        move_no_replace(&l.backup, &l.source)?;
        persist(&l, &mut j, "rolled_back")?;
        finish(&l, &j)?;
        return Ok(
            json!({"schema_version":1,"status":"rolled_back","mod_directory":display(root)}),
        );
    }
    if source_exists && l.source.is_dir() && backup_exists && j.phase == "publish_pending" {
        verify_original(&l.backup, &j)?;
        let records = read_manifest(&l, &j)?;
        validate_tree(&l.source, &records)?;
        persist(&l, &mut j, "committed")?;
        finish(&l, &j)?;
        return Ok(report(&l, &j, "committed", &records));
    }
    Err(error(
        "RECOVERY_REQUIRED",
        "转换路径状态冲突；已保留源、备份与临时目录",
    ))
}

#[cfg(test)]
thread_local! { static FAIL_AT: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
fn checkpoint(_name: &'static str) -> Result<()> {
    #[cfg(test)]
    if FAIL_AT.with(|f| *f.borrow() == Some(_name)) {
        return Err(error("INJECTED_FAILURE", _name));
    }
    Ok(())
}

fn unpack(root: &Path, source: &Path, progress: &mut dyn FnMut(&str, u8)) -> Result<Value> {
    // This handle denies external writes/deletion while StormLib is reading the original.
    let source_pin = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(source)
        .map_err(|e| error("SOURCE_IN_USE", format!("无法独占源写入权限：{e}")))?;
    let mut j = Journal {
        schema_version: 1,
        transaction_id: uuid::Uuid::new_v4().to_string(),
        mod_name: root.file_name().unwrap().to_string_lossy().into_owned(),
        archive_leaf: source.file_name().unwrap().to_string_lossy().into_owned(),
        archive_sha256: hash_file(source)?,
        archive_bytes: source_pin.metadata()?.len(),
        phase: "extracting".into(),
        manifest_sha256: None,
    };
    let l = layout(root, &j)?;
    progress("inspect", 2);
    let archive = Archive::open(source)?;
    let entries = archive.enumerate()?;
    let total = entries.iter().map(|e| e.record.size).sum::<u64>();
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    if available
        < total
            .saturating_add(entries.len() as u64 * 4096)
            .saturating_add(16 * 1024 * 1024)
    {
        return Err(error("INSUFFICIENT_SPACE", "解压所需磁盘空间不足"));
    }
    check_chain(&l.backup_dir)?;
    if !exists(&root.join("back"))? {
        fs::create_dir(root.join("back"))?;
    }
    fs::create_dir(&l.backup_dir)?;
    persist(&l, &mut j, "extracting")?;
    checkpoint("extracting")?;
    fs::create_dir(&l.stage)?;
    let payload = l.stage.join("payload");
    fs::create_dir(&payload)?;
    let records = archive.extract(entries, &payload, progress)?;
    drop(archive);
    progress("verify", 82);
    validate_tree(&payload, &records)?;
    write_new(
        &l.backup_dir.join("files.json"),
        &serde_json::to_vec(&records)?,
    )?;
    j.manifest_sha256 = Some(hash_file(&l.backup_dir.join("files.json"))?);
    persist(&l, &mut j, "prepared")?;
    checkpoint("prepared")?;
    verify_original(source, &j)?;
    persist(&l, &mut j, "backup_pending")?;
    checkpoint("backup_pending")?;
    drop(source_pin);
    move_no_replace(source, &l.backup)?;
    checkpoint("after_backup")?;
    let _backup_pin = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&l.backup)?;
    verify_original(&l.backup, &j)?;
    progress("backup", 90);
    persist(&l, &mut j, "publish_pending")?;
    checkpoint("publish_pending")?;
    move_no_replace(&payload, source)?;
    checkpoint("after_publish")?;
    persist(&l, &mut j, "committed")?;
    checkpoint("committed")?;
    finish(&l, &j)?;
    progress("complete", 100);
    Ok(report(&l, &j, "converted", &records))
}

pub(super) fn run(
    path: &Path,
    recovery: bool,
    progress: &mut dyn FnMut(&str, u8),
) -> Result<Value> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(error("INVALID_ARGUMENT", "请使用不含 . 或 .. 的绝对路径"));
    }
    let root = if recovery {
        path
    } else {
        path.parent()
            .ok_or_else(|| error("INVALID_ARGUMENT", "源路径没有父目录"))?
    };
    check_chain(root)?;
    let root = fs::canonicalize(root)?;
    if !root.is_dir() {
        return Err(error("INVALID_MOD_LAYOUT", "Mod 外层必须是目录"));
    }
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| error("INVALID_MOD_LAYOUT", "无效 Mod 名称"))?;
    paths::validate_relative(name)?;
    let leaf = if recovery {
        format!("{name}.mpq")
    } else {
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| error("INVALID_ARGUMENT", "无效 MPQ 文件名"))?
            .to_string()
    };
    if !leaf.eq_ignore_ascii_case(&format!("{name}.mpq")) {
        return Err(error(
            "INVALID_MOD_LAYOUT",
            "请先将源包放到 <名称>/<名称>.mpq；转换不改变 Mod 名称",
        ));
    }
    let source = root.join(leaf);
    let lock_path = root.join(".d2rhub-mpq.lock");
    exists(&lock_path)?;
    let _lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(lock_path)
        .map_err(|e| {
            error(
                "SOURCE_IN_USE",
                format!("另一个 MPQ 操作正在使用此 Mod：{e}"),
            )
        })?;
    let recovery_result = recover(&root).map_err(|e| error("RECOVERY_REQUIRED", e.message))?;
    if recovery {
        return Ok(recovery_result);
    }
    exists(&source)?;
    if source.is_dir() {
        list_tree(&source)?;
        validate_mod(&source)?;
        // Resuming a published transaction must retain its backup and escaped
        // names so callers can show the same diagnostics as a fresh conversion.
        if recovery_result["status"] == "committed" {
            return Ok(recovery_result);
        }
        return Ok(
            json!({"schema_version":1,"status":"already_directory","mod_name":name,"mod_directory":display(&root),"mpq_directory":display(&source)}),
        );
    }
    if !source.is_file() {
        return Err(error("INVALID_ARGUMENT", "未找到 MPQ 源文件"));
    }
    match unpack(&root, &source, progress) {
        Ok(report) => Ok(report),
        Err(e) => match recover(&root) {
            Ok(_) => Err(e),
            Err(r) => Err(error(
                "RECOVERY_REQUIRED",
                format!("{}；自动恢复失败：{}", e.message, r.message),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        workspace: PathBuf,
        root: PathBuf,
        source: PathBuf,
    }
    impl Fixture {
        fn new(listfile: bool, variant: u32) -> Self {
            let workspace =
                std::env::temp_dir().join(format!("d2r-mpq-test-{}", uuid::Uuid::new_v4()));
            let root = workspace.join("mini");
            fs::create_dir_all(&root).unwrap();
            let root = fs::canonicalize(root).unwrap();
            let source = root.join("mini.mpq");
            let wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
            assert!(unsafe {
                d2r_stormlib_sys::d2r_mpq_test_fixture(wide.as_ptr(), listfile, variant)
            });
            Self {
                workspace,
                root,
                source,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.workspace);
        }
    }
    #[test]
    fn mpq_conversion_preserves_backup_and_is_idempotent() {
        let f = Fixture::new(true, 4);
        let original = hash_file(&f.source).unwrap();
        let report = run(&f.source, false, &mut |_, _| {}).unwrap();
        assert_eq!(report["status"], "converted");
        assert_eq!(report["file_count"], 3);
        assert_eq!(report["escaped_file_names"].as_array().unwrap().len(), 1);
        assert_eq!(
            hash_file(Path::new(report["backup_path"].as_str().unwrap())).unwrap(),
            original
        );
        assert_eq!(
            fs::read(f.source.join("data/probe.bin")).unwrap(),
            b"test payload"
        );
        assert_eq!(
            fs::metadata(f.source.join("data/x-%CE%DE%BF%EC%BD%A8%B7%BF.json"))
                .unwrap()
                .len(),
            0
        );
        assert!(!f.source.join("(listfile)").exists());
        assert_eq!(
            run(&f.source, false, &mut |_, _| {}).unwrap()["status"],
            "already_directory"
        );
        assert_eq!(
            run(&f.root, true, &mut |_, _| {}).unwrap()["status"],
            "no_transaction"
        );
    }
    #[test]
    fn mpq_unpack_resumes_with_backup_and_escaped_name_report() {
        for phase in ["after_backup", "after_publish", "committed"] {
            let f = Fixture::new(true, 4);
            let original = hash_file(&f.source).unwrap();
            FAIL_AT.with(|p| *p.borrow_mut() = Some(phase));
            let interrupted = unpack(&f.root, &f.source, &mut |_, _| {});
            FAIL_AT.with(|p| *p.borrow_mut() = None);
            assert_eq!(interrupted.unwrap_err().code, "INJECTED_FAILURE");

            let report = run(&f.source, false, &mut |_, _| {}).unwrap();
            assert_eq!(
                report["status"],
                if phase == "after_backup" {
                    "converted"
                } else {
                    "committed"
                },
                "{phase}"
            );
            assert_eq!(report["mod_name"], "mini");
            assert_eq!(report["mod_directory"], display(&f.root));
            assert_eq!(report["mpq_directory"], display(&f.source));
            assert_eq!(report["archive_sha256"], original);
            assert_eq!(report["file_count"], 3);
            assert_eq!(
                hash_file(Path::new(report["backup_path"].as_str().unwrap())).unwrap(),
                original
            );
            let escaped = report["escaped_file_names"].as_array().unwrap();
            assert_eq!(escaped.len(), 1);
            assert_eq!(escaped[0]["path"], "data/x-%CE%DE%BF%EC%BD%A8%B7%BF.json");
            assert!(escaped[0]["archive_name_hex"].as_str().is_some());
            assert!(!f.root.join(JOURNAL).exists());
            assert_eq!(
                run(&f.source, false, &mut |_, _| {}).unwrap()["status"],
                "already_directory"
            );
        }
    }
    #[test]
    fn mpq_recovery_covers_every_commit_boundary() {
        for phase in [
            "extracting",
            "prepared",
            "backup_pending",
            "after_backup",
            "publish_pending",
            "after_publish",
            "committed",
        ] {
            let f = Fixture::new(true, 0);
            let original = hash_file(&f.source).unwrap();
            FAIL_AT.with(|p| *p.borrow_mut() = Some(phase));
            let result = unpack(&f.root, &f.source, &mut |_, _| {});
            FAIL_AT.with(|p| *p.borrow_mut() = None);
            assert_eq!(result.unwrap_err().code, "INJECTED_FAILURE", "{phase}");
            let report = recover(&f.root).unwrap();
            if matches!(phase, "after_publish" | "committed") {
                assert!(f.source.is_dir());
                assert_eq!(
                    hash_file(Path::new(report["backup_path"].as_str().unwrap())).unwrap(),
                    original
                );
            } else {
                assert_eq!(hash_file(&f.source).unwrap(), original, "{phase}");
            }
            assert_eq!(recover(&f.root).unwrap()["status"], "no_transaction");
        }
    }
    #[test]
    fn mpq_corrupt_published_tree_does_not_overwrite_or_discard_backup() {
        let f = Fixture::new(true, 0);
        FAIL_AT.with(|p| *p.borrow_mut() = Some("after_publish"));
        assert!(unpack(&f.root, &f.source, &mut |_, _| {}).is_err());
        FAIL_AT.with(|p| *p.borrow_mut() = None);
        fs::write(f.source.join("data/probe.bin"), "changed").unwrap();
        assert_eq!(recover(&f.root).unwrap_err().code, "RECOVERY_REQUIRED");
        assert!(f.root.join(JOURNAL).exists());
        assert!(f.source.is_dir());
        assert!(list_tree(&f.root.join("back"))
            .unwrap()
            .iter()
            .any(|p| p.file_name().unwrap() == "mini.mpq"));
    }
    #[test]
    fn mpq_rejects_unknown_names_bad_layout_paths_and_locale_without_mutation() {
        for (listfile, variant) in [(false, 0), (true, 1), (true, 2), (true, 5)] {
            let f = Fixture::new(listfile, variant);
            let original = hash_file(&f.source).unwrap();
            assert!(run(&f.source, false, &mut |_, _| {}).is_err());
            assert_eq!(hash_file(&f.source).unwrap(), original);
            assert!(!f.root.join(JOURNAL).exists());
        }
    }
    #[test]
    fn mpq_lock_excludes_second_converter() {
        let f = Fixture::new(true, 0);
        let _lock = OpenOptions::new()
            .create_new(true)
            .write(true)
            .share_mode(0)
            .open(f.root.join(".d2rhub-mpq.lock"))
            .unwrap();
        assert_eq!(
            run(&f.source, false, &mut |_, _| {}).unwrap_err().code,
            "SOURCE_IN_USE"
        );
    }
    #[test]
    fn mpq_commit_never_replaces_an_existing_file() {
        let f = Fixture::new(true, 0);
        let occupied = f.root.join("occupied.mpq");
        fs::write(&occupied, b"keep").unwrap();
        assert!(move_no_replace(&f.source, &occupied).is_err());
        assert!(f.source.is_file());
        assert_eq!(fs::read(&occupied).unwrap(), b"keep");
    }
}
