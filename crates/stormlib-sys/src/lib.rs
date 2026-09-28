//! Minimal ABI bindings to the pinned, vendored Windows x64 StormLib build.
#![allow(non_snake_case)]
use std::ffi::{c_char, c_void};
pub type Handle = *mut c_void;
#[repr(C)]
pub struct FindData {
    pub name: [c_char; 260],
    pub plain_name: *mut c_char,
    pub hash_index: u32,
    pub block_index: u32,
    pub size: u32,
    pub flags: u32,
    pub compressed_size: u32,
    pub time_lo: u32,
    pub time_hi: u32,
    pub locale: u32,
}
extern "C" {
    pub fn d2r_mpq_file_count(archive: Handle, count: *mut u32) -> bool;
}
extern "C" {
    pub fn d2r_mpq_test_fixture(path: *const u16, listfile: bool, variant: u32) -> bool;
}
extern "system" {
    pub fn SFileOpenArchive(
        path: *const u16,
        priority: u32,
        flags: u32,
        archive: *mut Handle,
    ) -> bool;
    pub fn SFileCloseArchive(archive: Handle) -> bool;
    pub fn SFileFindFirstFile(
        archive: Handle,
        mask: *const c_char,
        data: *mut FindData,
        listfile: *const u16,
    ) -> Handle;
    pub fn SFileFindNextFile(search: Handle, data: *mut FindData) -> bool;
    pub fn SFileFindClose(search: Handle) -> bool;
    pub fn SFileOpenFileEx(
        archive: Handle,
        name: *const c_char,
        scope: u32,
        file: *mut Handle,
    ) -> bool;
    pub fn SFileReadFile(
        file: Handle,
        buffer: *mut c_void,
        count: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> bool;
    pub fn SFileCloseFile(file: Handle) -> bool;
    pub fn SErrGetLastError() -> u32;
}
