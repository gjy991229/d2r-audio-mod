//! Independent hostile cue processing. No item/area processing is required.
use super::*;

pub(super) const ID: &str = "hostile_auto_restart";
pub(super) fn feature_group() -> ModFeatureGroup {
    ModFeatureGroup {
        id: ID.into(),
        recipe_version: 1,
        fingerprint: "hostile-restart-v1;control=1;esc=4".into(),
        reused_from_source: false,
    }
}

pub(super) fn validate_source(group: &ModFeatureGroup) -> Result<(), String> {
    let supported = feature_group();
    if group.recipe_version != supported.recipe_version
        || group.fingerprint != supported.fingerprint
    {
        return Err("来源敌对自动重开模块版本不受支持，不能降级覆盖".into());
    }
    Ok(())
}

pub(super) fn install(root: &Path, storage: Option<&casc_core::Storage>) -> Result<(), String> {
    let path = root.join("data/global/excel/sounds.txt");
    let text = if path.is_file() {
        read_excel_text(&path)?
    } else {
        read_casc_utf8(
            storage.ok_or("敌对自动重开需要游戏声音数据")?,
            "data/global/excel/sounds.txt",
        )?
    };
    let mut table = TsvTable::parse("sounds.txt", &text)?;
    let sound = table.column("Sound")?;
    let file = table.column("FileName")?;
    let cache = root.join("config/d2rhub-hostile-original");
    for name in [
        "cursor_hostile",
        "cursor_hostile_hd_1",
        "cursor_hostile_hd_2",
        "cursor_hostile_hd_3",
    ] {
        let row = table
            .rows
            .iter_mut()
            .find(|r| r[sound] == name)
            .ok_or_else(|| format!("声音表缺少 {name}"))?;
        let output = format!("d2rhub_hostile\\{name}.flac");
        let destination = root
            .join("data/hd/global/sfx")
            .join(output.replace('\\', "/"));
        if row[file] != output {
            // Current D2R installations retain the legacy row but may only ship
            // its HD replacement. Preserve the same hostile cue in that case.
            let source =
                resolve_audio_source(root, storage, &row[file], &cache, name).or_else(|error| {
                    if name == "cursor_hostile" {
                        resolve_audio_source(
                            root,
                            storage,
                            "cursor\\cursor_hostile_1_hd.flac",
                            &cache,
                            name,
                        )
                    } else {
                        Err(error)
                    }
                })?;
            write_marker_flac_with_source_gain(
                &destination,
                TelemetryMarker::Hostile,
                source.path.as_deref(),
                1.0,
                MarkerConfig {
                    gain_db: -18.0,
                    ..MarkerConfig::default()
                },
            )?;
            row[file] = output;
        } else if !destination.is_file() {
            return Err(format!(
                "敌对提示资源缺失：{}，请从原始 Mod 重新加工",
                destination.display()
            ));
        }
    }
    write_file(&path, table.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_relabel_future_or_unknown_hostile_signals() {
        let mut group = feature_group();
        assert!(validate_source(&group).is_ok());
        group.recipe_version += 1;
        assert!(validate_source(&group).is_err());
        group = feature_group();
        group.fingerprint = "unknown-control-frame".into();
        assert!(validate_source(&group).is_err());
    }
}
