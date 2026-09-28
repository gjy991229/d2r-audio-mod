//! Development helper for the personal LiteHubPlus builder. Reads only CASC.
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: litehub-casc-export GAME OUTPUT REQUESTS.json".into());
    }
    let paths: Vec<String> = serde_json::from_slice(&fs::read(&args[3])?)?;
    let storage = casc_core::Storage::open(&args[1])?;
    let output = PathBuf::from(&args[2]);
    let mut results = Vec::new();
    for path in paths {
        if !path.starts_with("data/")
            || path.contains(['\\', ':'])
            || Path::new(&path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe resource path: {path}").into());
        }
        match storage.read(&format!("data:{}", path.replace('/', "\\"))) {
            Ok(bytes) => {
                let target = output.join(&path);
                fs::create_dir_all(target.parent().ok_or("missing parent")?)?;
                fs::write(&target, &bytes)?;
                if path.ends_with(".json") {
                    let text = std::str::from_utf8(&bytes)?.trim_start_matches('\u{feff}');
                    let parsed: serde_json::Value = json5::from_str(text)?;
                    fs::write(
                        target.with_extension("parsed.json"),
                        serde_json::to_vec(&parsed)?,
                    )?;
                }
                results.push(serde_json::json!({"path":path,"available":true,"bytes":bytes.len()}));
            }
            Err(e) => results
                .push(serde_json::json!({"path":path,"available":false,"error":e.to_string()})),
        }
    }
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("export-results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
