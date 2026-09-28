use super::{error, paths, Result};
use d2r_stormlib_sys as sys;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    fs::OpenOptions,
    io::Write,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
};

pub(super) struct Archive(sys::Handle);
struct Search(sys::Handle);
struct Member(sys::Handle);
impl Drop for Archive {
    fn drop(&mut self) {
        unsafe {
            sys::SFileCloseArchive(self.0);
        }
    }
}
impl Drop for Search {
    fn drop(&mut self) {
        unsafe {
            sys::SFileFindClose(self.0);
        }
    }
}
impl Drop for Member {
    fn drop(&mut self) {
        unsafe {
            sys::SFileCloseFile(self.0);
        }
    }
}
fn check(ok: bool, action: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(error(
            "INVALID_MPQ",
            format!("{action}：StormLib error {}", unsafe {
                sys::SErrGetLastError()
            }),
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub archive_name_hex: String,
    pub escaped_name: bool,
}
pub(super) struct Entry {
    pub record: Record,
    raw: CString,
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut handle = ptr::null_mut();
        check(
            unsafe { sys::SFileOpenArchive(wide.as_ptr(), 0, 0x100, &mut handle) },
            "打开 MPQ 失败",
        )?;
        Ok(Self(handle))
    }
    pub fn enumerate(&self) -> Result<Vec<Entry>> {
        let mut expected = 0;
        check(
            unsafe { sys::d2r_mpq_file_count(self.0, &mut expected) },
            "读取条目总数失败",
        )?;
        if expected == 0 || expected > 1_000_000 {
            return Err(error("UNSUPPORTED_ARCHIVE", "MPQ 条目数超出支持范围"));
        }
        let mut data: sys::FindData = unsafe { std::mem::zeroed() };
        let handle =
            unsafe { sys::SFileFindFirstFile(self.0, c"*".as_ptr(), &mut data, ptr::null()) };
        check(!handle.is_null() && handle as isize != -1, "枚举 MPQ 失败")?;
        let _search = Search(handle);
        let mut entries = Vec::new();
        let mut indices = HashSet::new();
        let mut total = 0u64;
        loop {
            // Reject patch members and locale-dependent files instead of flattening semantics.
            if data.flags & 0x00100000 != 0 || data.locale != 0 {
                return Err(error(
                    "UNSUPPORTED_ARCHIVE",
                    "暂不转换补丁条目或非中性 locale 资源",
                ));
            }
            if !indices.insert(data.block_index) {
                return Err(error("UNSUPPORTED_ARCHIVE", "MPQ 条目别名无法无损转换"));
            }
            if !data.name.contains(&0) {
                return Err(error("UNSAFE_PATH", "文件名超出 StormLib 枚举长度"));
            }
            let raw = unsafe { CStr::from_ptr(data.name.as_ptr()) }.to_owned();
            let bytes = raw.as_bytes();
            let internal = [b"(listfile)".as_slice(), b"(attributes)", b"(signature)"]
                .iter()
                .any(|s| bytes.eq_ignore_ascii_case(s));
            if !internal {
                let (path, escaped_name) = paths::archive_name(bytes)?;
                total += u64::from(data.size);
                if total > 64 * 1024 * 1024 * 1024 {
                    return Err(error("UNSUPPORTED_ARCHIVE", "解压资源超过 64 GiB 支持上限"));
                }
                let archive_name_hex = hex(bytes);
                entries.push(Entry {
                    raw,
                    record: Record {
                        path,
                        size: data.size.into(),
                        sha256: String::new(),
                        archive_name_hex,
                        escaped_name,
                    },
                });
            }
            if !unsafe { sys::SFileFindNextFile(handle, &mut data) } {
                if unsafe { sys::SErrGetLastError() } != 18 {
                    return Err(error("INVALID_MPQ", "MPQ 枚举异常结束"));
                }
                break;
            }
        }
        if indices.len() != expected as usize {
            return Err(error("UNRESOLVED_ENTRIES", "MPQ 枚举未覆盖全部有效条目"));
        }
        paths::validate_collisions(entries.iter().map(|e| e.record.path.as_str()))?;
        if !entries
            .iter()
            .any(|e| e.record.path.eq_ignore_ascii_case("modinfo.json"))
            || !entries
                .iter()
                .any(|e| e.record.path.to_lowercase().starts_with("data/"))
        {
            return Err(error(
                "INVALID_MOD_LAYOUT",
                "MPQ 根目录必须直接包含 modinfo.json 和 data/资源，不能多套 Mod 外层目录",
            ));
        }
        entries.sort_by(|a, b| a.record.path.cmp(&b.record.path));
        Ok(entries)
    }
    pub fn extract(
        &self,
        entries: Vec<Entry>,
        destination: &Path,
        progress: &mut dyn FnMut(&str, u8),
    ) -> Result<Vec<Record>> {
        let length = entries.len();
        let mut records = Vec::with_capacity(length);
        let mut last_percent = 0;
        let mut buffer = vec![0u8; 256 * 1024];
        for (index, entry) in entries.into_iter().enumerate() {
            let mut handle = ptr::null_mut();
            check(
                unsafe { sys::SFileOpenFileEx(self.0, entry.raw.as_ptr(), 0, &mut handle) },
                "打开资源失败",
            )?;
            let _member = Member(handle);
            let output = destination.join(&entry.record.path);
            super::transaction::check_chain(&output)?;
            std::fs::create_dir_all(output.parent().unwrap())?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            let mut hash = Sha256::new();
            let mut remaining = entry.record.size;
            while remaining > 0 {
                let amount = remaining.min(buffer.len() as u64) as u32;
                let mut read = 0;
                check(
                    unsafe {
                        sys::SFileReadFile(
                            handle,
                            buffer.as_mut_ptr().cast(),
                            amount,
                            &mut read,
                            ptr::null_mut(),
                        )
                    },
                    "读取压缩资源失败",
                )?;
                if read != amount {
                    return Err(error("EXTRACT_FAILED", "资源解压长度不匹配"));
                }
                file.write_all(&buffer[..read as usize])?;
                hash.update(&buffer[..read as usize]);
                remaining -= u64::from(read);
            }
            let mut extra = 0;
            let read_more = unsafe {
                sys::SFileReadFile(
                    handle,
                    buffer.as_mut_ptr().cast(),
                    1,
                    &mut extra,
                    ptr::null_mut(),
                )
            };
            if read_more || extra != 0 || unsafe { sys::SErrGetLastError() } != 38 {
                return Err(error("EXTRACT_FAILED", "资源结束位置异常"));
            }
            file.sync_all()?;
            let mut record = entry.record;
            record.sha256 = hex(&hash.finalize());
            records.push(record);
            let percent = 10 + ((index + 1) * 70 / length) as u8;
            if percent != last_percent {
                progress("extract", percent);
                last_percent = percent;
            }
        }
        Ok(records)
    }
}
