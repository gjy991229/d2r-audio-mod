//! MYMOD's native per-player loot permission controls, without desktop automation.
use super::*;
use serde_json::{json, Value};

pub(super) const ID: &str = "party_loot";
const PANEL: &str = "D2RHubPartyLoot";
const FILE: &str = "data/global/ui/layouts/D2RHubPartyLoothd.json";
const JCY: &str = "data/global/ui/layouts/jcypartyextrahd.json";
const MYMOD: &str = "data/global/ui/layouts/mymodsShowListLayouthd.json";

pub(super) fn feature_group() -> ModFeatureGroup {
    ModFeatureGroup {
        id: ID.into(),
        recipe_version: 2,
        fingerprint: "party-loot-v2;native-party-row;jcy=1;launch=immediate".into(),
        reused_from_source: false,
    }
}

fn button() -> Value {
    json!({"type":"ToggleButtonWidget","name":"LootBodyButton","fields":{
        "rect":{"x":80,"y":20},"filename":"PANEL\\Party\\LootBody",
        "onClickMessage":"PartyPanelMessage:ToggleLootBody",
        "untoggledFrame":0,"untoggledPressedFrame":1,"untoggledDisabledFrame":2,
        "untoggledHoveredFrame":3,"toggledFrame":4,"toggledPressedFrame":5,"toggledHoveredFrame":6
    }})
}

fn install_button(document: &mut Value) -> Result<(), String> {
    if document["type"] != "PartyPanel" {
        return Err("左侧拾取按钮需要 PartyPanel 玩家列表".into());
    }
    let row = find_layout_node_mut(document, "FriendsListTemplate")
        .ok_or("玩家列表缺少 FriendsListTemplate")?;
    let children = row["children"]
        .as_array_mut()
        .ok_or("玩家列表行缺少 children")?;
    children.retain(|child| child["name"] != "LootBodyButton");
    children.push(button());
    Ok(())
}

fn adapt_jcy(panel: &mut Value) -> Result<(), String> {
    // JCY rows are 204 units apart (MYMOD: 174). Keep JCY's row origin and pitch.
    // Align the first button to MYMOD's known global x=161, y=280, reserving space before JCY's labels.
    let row = find_layout_node_mut(panel, "FriendsListTemplate").ok_or("JCY 缺少玩家行")?;
    let x = row["fields"]["rect"]["x"]
        .as_f64()
        .ok_or("JCY 玩家行横坐标无效")?;
    let y = row["fields"]["rect"]["y"]
        .as_f64()
        .ok_or("JCY 玩家行纵坐标无效")?;
    row["fields"]["rect"]["width"] = json!(300);
    let loot = find_layout_node_mut(row, "LootBodyButton").ok_or("JCY 缺少拾取按钮")?;
    loot["fields"]["rect"] = json!({"x":161.0-x,"y":280.0-y});
    for name in ["PlayerName", "PlayerInfo", "PartyInfoText"] {
        if let Some(text) = find_layout_node_mut(row, name) {
            let current = text["fields"]["rect"]["x"]
                .as_f64()
                .ok_or("JCY 玩家文字横坐标无效")?;
            text["fields"]["rect"]["x"] = json!(current.max(249.0 - x));
        }
    }
    panel["fields"]["priority"] = json!(-5);
    Ok(())
}

fn ensure_launcher(hud: &mut Value, name: &str) -> Result<(), String> {
    let message = format!("PanelManager:OpenPanel:{name}");
    let children = hud["children"]
        .as_array_mut()
        .ok_or("HudWarnings 缺少 children")?;
    let existing = children
        .iter()
        .find(|node| node["fields"]["message"] == message)
        .cloned();
    children.retain(|node| {
        node["name"] != "D2RHubPartyLootLauncher" && node["fields"]["message"] != message
    });
    let mut launcher = existing.unwrap_or_else(
        || json!({"type":"TimerWidget","name":"D2RHubPartyLootLauncher","fields":{}}),
    );
    if launcher["type"] != "TimerWidget" {
        return Err("玩家列表启动入口不是定时器，无法安全合并".into());
    }
    launcher["fields"]["time"] = json!(-1);
    launcher["fields"]["message"] = json!(message);
    children.insert(0, launcher);
    Ok(())
}

pub(super) fn install(
    root: &Path,
    storage: Option<&casc_core::Storage>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<(), String> {
    let mut hud = read_local_or_casc_json(root, storage, HUD_WARNINGS_LAYOUT)?;
    // Reuse MYMOD's existing roster so its player information and launcher remain intact.
    let existing = root.join(MYMOD).is_file();
    let jcy = !existing && root.join(JCY).is_file();
    let (file, name, mut panel) = if existing {
        (
            MYMOD,
            "mymodsShowListLayout",
            read_local_or_casc_json(root, storage, MYMOD)?,
        )
    } else if jcy {
        (
            JCY,
            "JcyPartyExtra",
            read_local_or_casc_json(root, storage, JCY)?,
        )
    } else {
        let storage = storage.ok_or("左侧拾取按钮需要 --game 原版游戏数据")?;
        let mut native = parse_json_value(&read_casc_utf8(
            storage,
            "data/global/ui/layouts/partypanelhd.json",
        )?)?;
        native["name"] = json!(PANEL);
        // Keep native named bindings, but display only the per-player loot control.
        native["fields"] = json!({"priority":-5,"fitToParent":true,
            "hostileColor":"$FontColorRed","yourPartyColor":"$FontColorPartyGreen",
            "inAnotherPartyColor":"$FontColorPartyOrange","unpartiedColor":"$FontColorWhite",
            "nonHardcoreSquelchXPosition":835,"hardcoreSquelchXPosition":885,"partyFrameFilename":""});
        let list = find_layout_node_mut(&mut native, "ListContainer")
            .ok_or("原生玩家列表缺少 ListContainer")?;
        list["fields"] =
            json!({"direction":"vertical","rect":{"x":81,"y":0,"width":300,"height":1878}});
        let row =
            find_layout_node_mut(list, "FriendsListTemplate").ok_or("原生玩家列表缺少行模板")?;
        row["fields"] =
            json!({"rect":{"x":0,"y":260,"width":160,"height":174},"focusOnMouseOver":false});
        for child in row["children"].as_array_mut().ok_or("原生玩家列表行无效")? {
            child["fields"]["visible"] = json!(false);
            child["fields"]["rect"] = json!({"x":-10000,"y":-10000});
        }
        for child in native["children"]
            .as_array_mut()
            .ok_or("原生玩家面板无效")?
        {
            if child["name"] != "ListContainer" {
                child["fields"]["visible"] = json!(false);
                child["fields"]["rect"] = json!({"x":-10000,"y":-10000});
            }
        }
        (FILE, PANEL, native)
    };
    install_button(&mut panel)?;
    if jcy {
        adapt_jcy(&mut panel)?;
    }
    ensure_launcher(&mut hud, name)?;
    write_json_layout(root, file, &panel)?;
    write_json_layout(root, HUD_WARNINGS_LAYOUT, &hud)?;
    compatibility.push(AudioModCompatibility {target:"左侧玩家尸体拾取按钮".into(), action:"add_party_loot".into(),
        detail:"使用游戏原生玩家行绑定和拾取权限按钮；点击某人的按钮切换该人的权限。仅高清键鼠界面，权限可用性由游戏决定。已有 MYMOD 或 JCY 列表原位合并，否则增加独立左侧列表。启动器立即执行，避免源 HUD 提前关闭导致按钮未加载。不绑定快捷键、不模拟点击。".into()});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jcy_keeps_its_row_spacing_and_launcher_precedes_early_hud_close() {
        let mut panel = json!({"type":"PartyPanel","fields":{"priority":-99},"children":[{
        "name":"ListContainer","children":[{"name":"FriendsListTemplate","fields":{"rect":{"x":46,"y":266,"width":1,"height":204}},"children":[
            {"name":"PlayerInfo","fields":{"rect":{"x":130,"y":10},"text":"keep"}},
            {"name":"LootBodyButton","fields":{"rect":{"x":9999,"y":9999}}}
        ]}]}]});
        install_button(&mut panel).unwrap();
        adapt_jcy(&mut panel).unwrap();
        let row = find_layout_node_mut(&mut panel, "FriendsListTemplate").unwrap();
        assert_eq!(row["fields"]["rect"]["height"], 204);
        assert_eq!(row["fields"]["rect"]["width"], 300);
        let loot = find_layout_node_mut(row, "LootBodyButton").unwrap();
        assert_eq!(loot["fields"]["rect"], json!({"x":115.0,"y":14.0}));
        let info = find_layout_node_mut(&mut panel, "PlayerInfo").unwrap();
        assert_eq!(info["fields"]["text"], "keep");
        assert_eq!(info["fields"]["rect"]["x"], 203.0);
        let mut hud = json!({"children":[
            {"type":"TimerWidget","name":"OpenJcyPartyExtra","fields":{"time":0.006,"message":"PanelManager:OpenPanel:JcyPartyExtra"}},
            {"type":"TimerWidget","name":"TimerClose","fields":{"time":0.008,"message":"PanelManager:ClosePanel:HUDWarnings"}},
            {"type":"TimerWidget","name":"D2RHubPartyLootLauncher","fields":{"time":1,"message":"PanelManager:OpenPanel:D2RHubPartyLoot"}}
        ]});
        ensure_launcher(&mut hud, "JcyPartyExtra").unwrap();
        ensure_launcher(&mut hud, "JcyPartyExtra").unwrap();
        assert_eq!(hud["children"].as_array().unwrap().len(), 2);
        assert_eq!(hud["children"][0]["fields"]["time"], -1);
        assert_eq!(
            hud["children"][0]["fields"]["message"],
            "PanelManager:OpenPanel:JcyPartyExtra"
        );
        assert_eq!(hud["children"][1]["fields"]["time"], 0.008);
    }
    #[test]
    fn merge_preserves_player_context_and_is_idempotent() {
        let mut panel = json!({"type":"PartyPanel","children":[{"name":"FriendsListTemplate","children":[{"name":"PlayerInfo","fields":{"text":"keep"}},button()]}]});
        install_button(&mut panel).unwrap();
        install_button(&mut panel).unwrap();
        let row = &panel["children"][0]["children"];
        assert_eq!(row.as_array().unwrap().len(), 2);
        assert_eq!(row[0]["fields"]["text"], "keep");
        assert_eq!(
            row[1]["fields"]["onClickMessage"],
            "PartyPanelMessage:ToggleLootBody"
        );
        assert!(install_button(&mut json!({"type":"Panel"})).is_err());
    }
}
