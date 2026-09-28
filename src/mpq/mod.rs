//! Explicit source conversion; augment keeps its existing read-only source contract.
#[cfg(windows)]
mod archive;
mod paths;
#[cfg(windows)]
mod transaction;
use serde_json::{json, Value};
use std::{ffi::OsString, io::Write, path::PathBuf};
#[derive(Debug)]
pub(super) struct Error {
    pub code: &'static str,
    pub message: String,
}
pub(super) type Result<T> = std::result::Result<T, Error>;
pub(super) fn error(code: &'static str, message: impl Into<String>) -> Error {
    Error {
        code,
        message: message.into(),
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        error("IO_ERROR", e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        error("INVALID_METADATA", e.to_string())
    }
}
pub(super) fn emit(value: Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}
pub fn capabilities(args: &[OsString]) -> std::result::Result<(), String> {
    if args != [OsString::from("--json")] {
        return Err("用法：capabilities --json".into());
    }
    let supported: &[&str] = if cfg!(windows) {
        &["mpq_unpack_v1", "mpq_recover_v1"]
    } else {
        &[]
    };
    emit(json!({"schema_version":1,"capabilities":supported}));
    Ok(())
}
pub fn cli(args: &[OsString], recover: bool) -> std::result::Result<(), String> {
    if args == [OsString::from("--licenses")] {
        println!(
            "{}",
            include_str!("../../crates/stormlib-sys/THIRD_PARTY_NOTICES.md")
        );
        return Ok(());
    }
    if args == [OsString::from("--help")] || args == [OsString::from("-h")] {
        println!(
            "{}",
            if recover {
                "recover-mpq --mod-directory <绝对路径/name> [--events|--json]"
            } else {
                "unpack-mpq --source <绝对路径/name/name.mpq> [--events|--json]\n原包备份至 name/back/<事务ID>/name.mpq，原路径转为目录；不会加工或改名。"
            }
        );
        return Ok(());
    }
    let events = args.iter().any(|a| a == "--events");
    let operation = if recover { "recover_mpq" } else { "unpack_mpq" };
    let result = (|| -> Result<Value> {
        let flag = if recover {
            "--mod-directory"
        } else {
            "--source"
        };
        let mut path = None;
        let mut json_output = false;
        let mut seen_events = false;
        let mut i = 0;
        while i < args.len() {
            if args[i] == flag && path.is_none() {
                i += 1;
                path = Some(PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| error("INVALID_ARGUMENT", "路径参数缺失"))?,
                ));
            } else if args[i] == "--events" && !seen_events {
                seen_events = true;
            } else if args[i] == "--json" && !json_output {
                json_output = true;
            } else {
                return Err(error(
                    "INVALID_ARGUMENT",
                    format!("不支持或重复的参数：{}", args[i].to_string_lossy()),
                ));
            }
            i += 1;
        }
        if json_output && events {
            return Err(error("INVALID_ARGUMENT", "--json 与 --events 不能同时使用"));
        }
        let path =
            path.ok_or_else(|| error("INVALID_ARGUMENT", format!("必须提供 {flag} <绝对路径>")))?;
        #[cfg(windows)]
        {
            transaction::run(&path, recover, &mut |phase, percent| {
                if events {
                    emit(
                        json!({"type":"progress","operation":operation,"phase":phase,"percent":percent,"message":phase}),
                    );
                }
            })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err(error(
                "UNSUPPORTED_PLATFORM",
                "MPQ 转换目前仅支持 Windows x64",
            ))
        }
    })();
    match result {
        Ok(report) => {
            if events {
                emit(json!({"type":"completed","operation":operation,"report":report}));
            } else {
                emit(report);
            }
            Ok(())
        }
        Err(e) => {
            if events {
                emit(
                    json!({"type":"error","operation":operation,"code":e.code,"message":e.message,"recovery_required":e.code == "RECOVERY_REQUIRED"}),
                );
            }
            Err(format!("{}: {}", e.code, e.message))
        }
    }
}
