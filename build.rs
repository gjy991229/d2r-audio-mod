use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    const VARIABLE: &str = "D2RHUB_INVENTORY_PRESET";
    const LOCAL_PRESET: &str = "resources/private-inventory.preset.gz";
    println!("cargo:rerun-if-env-changed={VARIABLE}");
    let configured = env::var_os(VARIABLE);
    let input = configured
        .as_ref()
        .map_or_else(|| PathBuf::from(LOCAL_PRESET), PathBuf::from);
    println!("cargo:rerun-if-changed={}", input.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("inventory.preset.gz");
    if configured.is_some() || input.is_file() {
        fs::copy(input, output)?;
    } else {
        fs::write(output, [])?;
    }
    Ok(())
}
