// Standalone Rust ABI/link test; not included in the processor Cargo build.
use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

#[link(name = "StormLib", kind = "static")]
#[link(name = "user32")]
extern "system" {
    fn SFileOpenArchive(path: *const u16, priority: u32, flags: u32, archive: *mut *mut c_void) -> bool;
    fn SFileOpenFileEx(archive: *mut c_void, name: *const u8, scope: u32, file: *mut *mut c_void) -> bool;
    fn SFileReadFile(file: *mut c_void, buffer: *mut c_void, count: u32, read: *mut u32, overlapped: *mut c_void) -> bool;
    fn SFileCloseFile(file: *mut c_void) -> bool;
    fn SFileCloseArchive(archive: *mut c_void) -> bool;
}

fn main() {
    let path = std::env::args_os().nth(1).expect("pass fixture MPQ path");
    let wide: Vec<u16> = path.encode_wide().chain(Some(0)).collect();
    let mut archive = ptr::null_mut();
    let mut file = ptr::null_mut();
    unsafe {
        assert!(SFileOpenArchive(wide.as_ptr(), 0, 0x100, &mut archive), "open archive");
        let opened = SFileOpenFileEx(archive, b"modinfo.json\0".as_ptr(), 0, &mut file);
        if !opened { SFileCloseArchive(archive); panic!("open file"); }
        let expected = b"{\"name\":\"Probe\",\"savepath\":\"Probe/\"}";
        let mut bytes = vec![0u8; expected.len()];
        let mut read = 0;
        let ok = SFileReadFile(file, bytes.as_mut_ptr().cast(), bytes.len() as u32, &mut read, ptr::null_mut());
        let file_closed = SFileCloseFile(file);
        let archive_closed = SFileCloseArchive(archive);
        assert!(ok && file_closed && archive_closed);
        assert_eq!(read as usize, expected.len());
        assert_eq!(&bytes, expected);
    }
    println!("PASS Rust static-link FFI + Unicode archive path + exact resource bytes");
}
