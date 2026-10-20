//! Exact snapshot of the user's enabled, working MYMOD chain. HUD merging is separate.
use super::*;
use serde_json::{json, Value};
pub(super) const ID: &str = "scene_shortcuts";
const FINGERPRINT: &str =
    "scene-shortcuts-v7;mymod-exact-chain;actinfo=a3-a4-waypoints;hud=unload-then-arrival;arrival_ms=1000;help_only=1";
const BACKUP: &str = "d2rhub-scene-shortcuts-original-help.json";
const FILES: &[(&str, &[u8])] = &[
    (
        "helppanelhd.json",
        include_bytes!("scene_templates/mymod_exact/helppanelhd.json"),
    ),
    (
        "helppanel.json",
        include_bytes!("scene_templates/mymod_exact/helppanel.json"),
    ),
    (
        "waypointspaneloriginalhd.json",
        include_bytes!("scene_templates/mymod_exact/waypointspaneloriginalhd.json"),
    ),
    (
        "CALL/OnekeyTravincalhd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/OnekeyTravincalhd.json"),
    ),
    (
        "CALL/WP-t-autuexitonekeyhd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/WP-t-autuexitonekeyhd.json"),
    ),
    (
        "CALL/WPautuexitonekeyhd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/WPautuexitonekeyhd.json"),
    ),
    (
        "CALL/HJWPautuexitonekeyhd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/HJWPautuexitonekeyhd.json"),
    ),
    (
        "CALL/helppanelexithd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/helppanelexithd.json"),
    ),
    (
        "CALL/helppanelonekeyhd.json",
        include_bytes!("scene_templates/mymod_exact/CALL/helppanelonekeyhd.json"),
    ),
    (
        "QuickHellGamePanelhd.json",
        include_bytes!("scene_templates/mymod_exact/QuickHellGamePanelhd.json"),
    ),
];
pub(super) fn feature_group() -> ModFeatureGroup {
    ModFeatureGroup {
        id: ID.into(),
        recipe_version: 7,
        fingerprint: FINGERPRINT.into(),
        reused_from_source: false,
    }
}
fn install_launcher(hud: &mut Value) -> Result<(), String> {
    let children = hud["children"]
        .as_array_mut()
        .ok_or("HudWarnings 缺少 children")?;
    children.retain(|n| {
        let message = n["fields"]["message"].as_str().unwrap_or("");
        !n["name"].as_str().unwrap_or("").starts_with("D2RHubScene")
            && !message.contains("D2RHubScene")
            && message != "PanelManager:OpenPanel:CALL/OnekeyTravincal"
            && !message.eq_ignore_ascii_case("PanelManager:ClosePanel:HUDWarnings")
            && !message.eq_ignore_ascii_case("PanelManager:UnloadPanel:HUDWarnings")
    });
    // JCY opens another HUDWarningsPanel at 1ms; match MYMOD's 1s clone startup.
    for n in children.iter_mut() {
        if n["fields"]["message"] == "PanelManager:OpenPanel:JcyHudWarnings" {
            n["fields"]["time"] = json!(1);
        }
    }
    // Exact actions/order/time from the enabled MYMOD HUDWarnings.
    children.push(json!({"type":"TimerWidget","name":"TimerClose10","fields":{"time":1,"message":"PanelManager:UnloadPanel:HUDWarnings"}}));
    children.push(json!({"type":"TimerWidget","name":"TimerClose7Center","fields":{"time":1,"message":"PanelManager:OpenPanel:CALL/OnekeyTravincal"}}));
    Ok(())
}
fn patch_actinfo(source: &str) -> Result<String, String> {
    let mut rows: Vec<Vec<String>> = source
        .lines()
        .map(|line| {
            line.trim_end_matches('\r')
                .split('\t')
                .map(str::to_string)
                .collect()
        })
        .collect();
    let reference: Vec<Vec<&str>> = include_str!("scene_templates/mymod_exact/actinfo.txt")
        .lines()
        .map(|l| l.trim_end_matches('\r').split('\t').collect())
        .collect();
    let header = rows.first().ok_or("actinfo.txt 为空")?;
    let column = |name: &str| -> Result<usize, String> {
        let matches: Vec<_> = header
            .iter()
            .enumerate()
            .filter(|(_, h)| h.trim_start_matches('\u{feff}') == name)
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 {
            return Err(format!("actinfo.txt 列缺失或重复：{name}"));
        }
        Ok(matches[0])
    };
    let act_column = column("act")?;
    let columns: Vec<_> = (1..=9)
        .map(|i| column(&format!("waypoint{i}")))
        .collect::<Result<_, _>>()?;
    for act in ["3", "4"] {
        let matches: Vec<_> = rows
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, row)| row.get(act_column).is_some_and(|v| v == act))
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 {
            return Err(format!("actinfo.txt 幕编号缺失或重复：{act}"));
        }
        let target = &mut rows[matches[0]];
        let original = reference
            .iter()
            .skip(1)
            .find(|r| r.first() == Some(&act))
            .ok_or("参考 actinfo 幕编号缺失")?;
        for (i, target_column) in columns.iter().enumerate() {
            let reference_column = reference[0]
                .iter()
                .position(|h| *h == format!("waypoint{}", i + 1))
                .ok_or("参考 actinfo 列缺失")?;
            if target.len() <= *target_column {
                return Err(format!("actinfo.txt 幕{act}行不完整"));
            }
            target[*target_column] = original[reference_column].to_string();
        }
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = rows
        .into_iter()
        .map(|r| r.join("\t"))
        .collect::<Vec<_>>()
        .join(newline);
    if source.ends_with('\n') {
        output.push_str(newline);
    }
    Ok(output)
}

pub(super) fn install(
    root: &Path,
    storage: Option<&casc_core::Storage>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<(), String> {
    let table_path = root.join("data/global/excel/actinfo.txt");
    let table = if table_path.exists() {
        std::fs::read_to_string(&table_path).map_err(|e| e.to_string())?
    } else {
        read_casc_utf8(
            storage.ok_or("场景重开需要 actinfo.txt 或原版游戏数据")?,
            "data/global/excel/actinfo.txt",
        )?
    };
    let patched_table = patch_actinfo(&table)?;
    let mut hud = read_local_or_casc_json(root, storage, HUD_WARNINGS_LAYOUT)?;
    let help = read_local_or_casc_json(root, storage, "data/global/ui/layouts/helppanelhd.json")?;
    install_launcher(&mut hud)?;
    if !root.join(BACKUP).exists() {
        write_json_layout(root, BACKUP, &help)?;
    }
    for (relative, bytes) in FILES {
        let path = root.join(UI_LAYOUTS_DIRECTORY).join(relative);
        let backup = root.join("d2rhub-scene-original").join(relative);
        if path.exists() && !backup.exists() {
            std::fs::create_dir_all(backup.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(&path, backup).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    for name in [
        "Restart",
        "Waypoint",
        "Recreate",
        "Exit",
        "LoadHell",
        "Toolbar",
        "Arrival",
        "ArrivalWaypoint",
        "Entry",
    ] {
        let path = root
            .join(UI_LAYOUTS_DIRECTORY)
            .join(format!("D2RHubScene{name}hd.json"));
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    let table_backup = root.join("d2rhub-scene-original/global/excel/actinfo.txt");
    if !table_backup.exists() {
        std::fs::create_dir_all(table_backup.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(table_backup, &table).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(table_path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(table_path, patched_table).map_err(|e| e.to_string())?;
    write_json_layout(root, HUD_WARNINGS_LAYOUT, &hud)?;
    compatibility.push(AudioModCompatibility { target:"场景重开".into(), action:"add_scene_shortcuts".into(),
        detail:"需在 A3、崔凡克或 A4 小站处重开或创房，使用游戏原生小站与房间消息链，无需图像识别。同步第三、四幕小站列表映射并保留参考面板时序；帮助键可触发下一局地狱，被替换的帮助页与布局均保留备份。".into() });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actinfo_mapping_is_required_idempotent_and_preserves_other_data() {
        let original = include_str!("scene_templates/mymod_exact/actinfo.txt");
        let mut rows: Vec<Vec<String>> = original
            .lines()
            .map(|l| l.split('\t').map(str::to_string).collect())
            .collect();
        let wp2 = rows[0].iter().position(|x| x == "waypoint2").unwrap();
        let a3 = rows.iter().position(|r| r[0] == "3").unwrap();
        rows[a3][wp2] = "Act 3 - Jungle 1".into();
        rows[0].push("custom-column".into());
        for row in rows.iter_mut().skip(1) {
            row.push("keep-this".into());
        }
        let source = rows
            .iter()
            .map(|r| r.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let patched = patch_actinfo(&source).unwrap();
        assert!(patched.contains("Act 3 - Town\tAct 4 - Town\tAct 3 - Travincal"));
        assert_eq!(patch_actinfo(&patched).unwrap(), patched);
        let parsed: Vec<Vec<&str>> = patched.lines().map(|l| l.split('\t').collect()).collect();
        for (i, row) in parsed.iter().enumerate().skip(1) {
            assert_eq!(row.last(), Some(&"keep-this"));
            if !["3", "4"].contains(&row[0]) {
                assert_eq!(
                    row.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    rows[i]
                );
            }
        }
        let duplicated = source.clone() + &rows[a3].join("\t") + "\n";
        assert!(patch_actinfo(&duplicated).is_err());
    }
    #[test]
    fn jcy_hud_merge_matches_reference_and_is_idempotent() {
        let mut hud = json!({"children":[
            {"type":"TimerWidget","name":"Party","fields":{"time":-1,"message":"PanelManager:OpenPanel:JcyPartyExtra"}},
            {"type":"TimerWidget","name":"Clone","fields":{"time":0.001,"message":"PanelManager:OpenPanel:JcyHudWarnings"}},
            {"type":"TimerWidget","name":"OldClose","fields":{"time":0.008,"message":"PanelManager:ClosePanel:HUDWarnings"}},
            {"type":"TimerWidget","name":"D2RHubSceneEntryLauncher","fields":{"time":-1,"message":"PanelManager:OpenPanel:D2RHubSceneEntry"}}
        ]});
        install_launcher(&mut hud).unwrap();
        let once = hud.clone();
        install_launcher(&mut hud).unwrap();
        assert_eq!(once, hud);
        assert_eq!(hud["children"][0]["fields"]["time"], -1);
        assert_eq!(hud["children"][1]["fields"]["time"], 1);
        assert_eq!(
            hud["children"][2]["fields"]["message"],
            "PanelManager:UnloadPanel:HUDWarnings"
        );
        assert_eq!(
            hud["children"][3]["fields"]["message"],
            "PanelManager:OpenPanel:CALL/OnekeyTravincal"
        );
        assert_eq!(hud["children"].as_array().unwrap().len(), 4);
    }
    #[test]
    fn exact_reference_files_parse_without_rewriting() {
        for (name, bytes) in FILES {
            assert!(
                parse_json_value(std::str::from_utf8(bytes).unwrap()).is_ok(),
                "{name}"
            );
        }
    }
}
