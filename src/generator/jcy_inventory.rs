//! Install the previously prepared JCY-style inventory and stash as one appearance.
use super::*;
use flate2::read::GzDecoder;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

// Retain feature identity for saved Hub jobs and upgrade the two native reconstructions.
pub(super) const ID: &str = "jcy_inventory";
const RECIPE: u32 = 4;
const FINGERPRINT: &str = "inventory-stash-v4;reference=jcy;cube=embedded;assets=packaged";
const V1_FINGERPRINT: &str = "jcy-inventory-v1;layout=replace;cube=embedded";
const V2_FINGERPRINT: &str = "inventory-stash-v2;base=game;cube=side-panel;stash=direct-pages";
const V3_FINGERPRINT: &str = "inventory-stash-v3;base=game;cube=embedded;stash=vertical-pages";
const LAYOUTS: &str = "data/global/ui/layouts";
const BAG: &str = "playerinventoryexpansionlayouthd.json";
const BANK: &str = "bankexpansionlayouthd.json";
const CUBE: &str = "JcyMiniCubehd.json";
const CUBE_CLOSE: &str = "JcyMiniCubeClosehd.json";
const ALTERNATE_BAG: &str = "my1playerinventoryexpansionlayouthd.json";
const PROFILE: &str = "data/global/ui/layouts/_profilehd.json";
const INVENTORY: &str = "data/global/excel/inventory.txt";
const UI_SPRITES: &str = "data/hd/global/ui";
const SPRITE_PREFIX: &str = "d2rhub/jcy_inventory/";
const STRINGS: &str = "data/local/lng/strings";
const UI_STRINGS: &str = "data/local/lng/strings/ui.json";
const STYLE_PREFIX: &str = "D2RHubJcyInventory";
const CUBE_LAUNCH_ACTIONS: &[&str] = &["openpanel", "togglepanel", "loadpanel"];
const OLD_CUBE_PANELS: &[&str] = &[
    "horadriccubelayoutmini",
    "horadriccubelayoutminicenter",
    "jcyminicube",
    "jcyminicubeclose",
    "d2rhubinventorycube",
    "d2rhubinventorycubeclose",
];
const NATIVE_FALLBACKS: &[&str] = &[
    "playerinventoryoriginallayouthd.json",
    "bankoriginallayouthd.json",
];
const PACKED_PRESET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/inventory.preset.gz"));

#[derive(Deserialize)]
struct Preset {
    bag: Value,
    bank: Value,
    bank_nine: Value,
    cube: Value,
    close: Value,
    pages: Vec<Value>,
    styles: BTreeMap<String, Value>,
    labels: Vec<Value>,
    sprites: Vec<(String, usize)>,
}

pub(crate) fn preset_available() -> bool {
    !PACKED_PRESET.is_empty()
}

fn read_preset() -> Result<(Preset, Vec<u8>), String> {
    if !preset_available() {
        return Err("此加工器未内置 JCY 背包与仓库预设，请使用带预设的构建".into());
    }
    let mut reader = GzDecoder::new(PACKED_PRESET).take(64 * 1024 * 1024 + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取内置布局预设失败：{e}"))?;
    let invalid = || "内置 JCY 布局预设不完整".to_string();
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid());
    }
    let header_len =
        u32::from_le_bytes(bytes.get(..4).ok_or_else(invalid)?.try_into().unwrap()) as usize;
    let header_end = 4 + header_len;
    let manifest = bytes.get(4..header_end).ok_or_else(invalid)?;
    let preset: Preset =
        serde_json::from_slice(manifest).map_err(|e| format!("解析内置布局预设失败：{e}"))?;
    let sprites = bytes.split_off(header_end);
    Ok((preset, sprites))
}

fn feature_group(reused_from_source: bool) -> ModFeatureGroup {
    ModFeatureGroup {
        id: ID.into(),
        recipe_version: RECIPE,
        fingerprint: FINGERPRINT.into(),
        reused_from_source,
    }
}

fn validate_source(group: &ModFeatureGroup) -> Result<(), String> {
    match (group.recipe_version, group.fingerprint.as_str()) {
        (RECIPE, FINGERPRINT) | (1, V1_FINGERPRINT) | (2, V2_FINGERPRINT) | (3, V3_FINGERPRINT) => {
            Ok(())
        }
        _ => Err("来源背包与仓库配方不受支持，无法加工".into()),
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    parse_json_value(&read_utf8(path)?)
        .map_err(|err| format!("解析 {} 失败：{err}", path.display()))
}

fn read_layout(mpq: &Path, file: &str) -> Result<Value, String> {
    read_json(&mpq.join(LAYOUTS).join(file))
}

fn write_layout(mpq: &Path, file: &str, value: &Value) -> Result<(), String> {
    write_json_layout(mpq, &format!("{LAYOUTS}/{file}"), value)
}

fn check_capacity(mpq: &Path) -> Result<(), String> {
    let path = mpq.join(INVENTORY);
    if !path.is_file() {
        return Ok(());
    }
    let table = TsvTable::parse(INVENTORY, &read_excel_text(&path)?)?;
    check_table_capacity(&table)
}

fn check_table_capacity(table: &TsvTable) -> Result<(), String> {
    let (class, x, y) = (
        table.column("class")?,
        table.column("gridX")?,
        table.column("gridY")?,
    );
    for row in &table.rows {
        let name = row[class].to_ascii_lowercase();
        let expected = match name.as_str() {
            "big bank page 1" | "big bank page2" => (10, 10),
            "transmogrify box page 1" | "transmogrify box2" => (3, 4),
            _ if [
                "amazon",
                "sorceress",
                "necromancer",
                "paladin",
                "barbarian",
                "druid",
                "assassin",
                "warlock",
            ]
            .contains(&name.strip_suffix('2').unwrap_or(name.as_str())) =>
            {
                (10, 4)
            }
            _ => continue,
        };
        if (row[x].trim().parse().ok(), row[y].trim().parse().ok())
            != (Some(expected.0), Some(expected.1))
        {
            return Err(format!(
                "JCY 参考贴图需要 {}×{} 的物品格；源 Mod 的 inventory.txt 将 {} 设为 {}×{}，无法完整显示",
                expected.0, expected.1, row[class], row[x], row[y]
            ));
        }
    }
    Ok(())
}

fn replace_strings(value: &mut Value, visitor: &mut impl FnMut(&mut String)) {
    match value {
        Value::String(text) => visitor(text),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| replace_strings(value, visitor)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| replace_strings(value, visitor)),
        _ => {}
    }
}

fn json_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() && recursive {
            files.extend(json_files(&entry.path(), true)?);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn old_cube_action(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [manager, action, panel] => {
            manager.eq_ignore_ascii_case("PanelManager")
                && CUBE_LAUNCH_ACTIONS
                    .iter()
                    .any(|name| action.eq_ignore_ascii_case(name))
                && OLD_CUBE_PANELS
                    .iter()
                    .any(|name| panel.eq_ignore_ascii_case(name))
        }
        _ => false,
    }
}

fn disable_old_cube(widget: &mut Value, startup_hud: bool) -> bool {
    let Some(children) = widget.get_mut("children").and_then(Value::as_array_mut) else {
        return false;
    };
    let before = children.len();
    children.retain(|child| {
        let old_launcher = child["fields"].as_object().is_some_and(|fields| {
            fields
                .values()
                .filter_map(Value::as_str)
                .any(old_cube_action)
        });
        let companion_close = startup_hud
            && matches!(child["name"].as_str(), Some("TimerClose6" | "TimerClose8"))
            && child["fields"]["message"] == "PlayerInventoryPanelMessage:Close";
        !(old_launcher || companion_close)
    });
    let mut changed = children.len() != before;
    for child in children {
        changed |= disable_old_cube(child, startup_hud);
    }
    changed
}

fn remove_old_layouts(mpq: &Path) -> Result<(), String> {
    for file in NATIVE_FALLBACKS {
        let path = mpq.join(LAYOUTS).join(file);
        if path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("清除旧布局 {} 失败：{e}", path.display()))?;
        }
    }
    for path in json_files(&mpq.join(LAYOUTS), true)? {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if [BAG, BANK, ALTERNATE_BAG, CUBE, CUBE_CLOSE]
            .iter()
            .any(|file| name.eq_ignore_ascii_case(file))
        {
            continue;
        }
        let text = read_utf8(&path)?;
        let startup_hud = ["hudwarningshd.json", "hudwarningskbhd.json"]
            .iter()
            .any(|file| name.eq_ignore_ascii_case(file));
        let lowered = text.to_ascii_lowercase();
        let mentions_launch = CUBE_LAUNCH_ACTIONS.iter().any(|action| {
            OLD_CUBE_PANELS
                .iter()
                .any(|panel| lowered.contains(&format!("panelmanager:{action}:{panel}")))
        });
        let mentions_companion = startup_hud
            && lowered.contains("playerinventorypanelmessage:close")
            && (lowered.contains("timerclose6") || lowered.contains("timerclose8"));
        if !mentions_launch && !mentions_companion {
            continue;
        }
        let mut layout = parse_json_value(&text)
            .map_err(|e| format!("无法检查旧迷你方盒入口 {}：{e}", path.display()))?;
        if disable_old_cube(&mut layout, startup_hud) {
            let relative = path
                .strip_prefix(mpq)
                .map_err(|e| e.to_string())?
                .to_string_lossy();
            write_json_layout(mpq, &relative, &layout)?;
        }
    }
    Ok(())
}

fn page_count(bank: &Value) -> usize {
    let count = bank["name"].as_str().is_some_and(|name| {
        name.strip_prefix("page_button_")
            .is_some_and(|n| n.parse::<usize>().is_ok())
    });
    usize::from(count)
        + bank["children"]
            .as_array()
            .map_or(0, |nodes| nodes.iter().map(page_count).sum())
}

fn adjust_page_count(bank: &mut Value, count: usize) -> Result<(), String> {
    if count != 10 {
        return Ok(());
    }
    let nodes = find_layout_node_mut(bank, "StashNavigation")
        .and_then(|nav| nav["children"].as_array_mut())
        .ok_or("预设缺少仓库分页")?;
    nodes.retain(|node| node["name"] != "page_button_11");
    for page in 1..=count {
        let name = format!("page_button_{page}");
        let button = nodes
            .iter_mut()
            .find(|node| node["name"] == name)
            .ok_or("预设缺少直达按钮")?;
        button["fields"]["rect"]["y"] =
            json!(-980 + (784.0 * (page - 1) as f64 / (count - 1) as f64).round() as i64);
    }
    Ok(())
}

fn install_sprites(mpq: &Path, sprites: &[(String, usize)], bytes: &[u8]) -> Result<(), String> {
    let mut remaining = bytes;
    for (path, len) in sprites {
        let stem = path
            .strip_prefix(SPRITE_PREFIX)
            .and_then(|path| {
                path.strip_suffix(".lowend.sprite")
                    .or_else(|| path.strip_suffix(".sprite"))
            })
            .ok_or("内置 JCY 贴图路径无效")?;
        if stem.split('/').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
        }) {
            return Err(format!("内置 JCY 贴图路径无效：{path}"));
        }
        let (sprite, tail) = remaining
            .split_at_checked(*len)
            .filter(|(item, _)| !item.is_empty())
            .ok_or("内置 JCY 贴图内容不完整")?;
        write_file(&mpq.join(UI_SPRITES).join(path), sprite)?;
        remaining = tail;
    }
    if !remaining.is_empty() {
        return Err("内置 JCY 贴图清单长度不符".into());
    }
    Ok(())
}

fn install_profile(
    mpq: &Path,
    storage: Option<&casc_core::Storage>,
    styles: BTreeMap<String, Value>,
    bag: &mut Value,
    bank: &mut Value,
    cube: &mut Value,
) -> Result<(), String> {
    let mut profile = read_local_or_casc_json(mpq, storage, PROFILE)?;
    let fields = profile.as_object_mut().ok_or("目标 Mod 的界面参数无效")?;
    let mut scoped = BTreeMap::new();
    let changed_cell_size = styles
        .get("ItemCellSize")
        .is_some_and(|size| fields.get("ItemCellSize") != Some(size));
    for (key, value) in styles {
        if key.starts_with(STYLE_PREFIX) {
            fields.insert(key, value);
        } else if fields.get(&key) != Some(&value) {
            let local = format!("{STYLE_PREFIX}{key}");
            scoped.insert(format!("${key}"), format!("${local}"));
            fields.insert(local, value);
        }
    }
    if changed_cell_size {
        for child in bag["children"].as_array_mut().ok_or("预设缺少背包控件")? {
            if matches!(
                child["type"].as_str(),
                Some("InventoryGridWidget" | "InventorySlotWidget")
            ) {
                child["fields"]["cellSize"] = json!("$ItemCellSize");
            }
        }
    }
    for layout in [bag, bank, cube] {
        replace_strings(layout, &mut |text| {
            if let Some(scoped) = scoped.get(text) {
                text.clone_from(scoped);
            }
        });
    }
    write_json_layout(mpq, PROFILE, &profile)
}

fn register_label(row: &Value, keys: &mut BTreeSet<String>, ids: &mut BTreeSet<u64>) {
    if let Some(key) = row["Key"].as_str() {
        keys.insert(key.to_owned());
    }
    if let Some(id) = row["id"].as_u64() {
        ids.insert(id);
    }
}

fn label_rows(doc: &Value) -> Result<&[Value], String> {
    doc.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| "目标 Mod 的语言文件不是词条数组".into())
}

fn install_labels(
    mpq: &Path,
    storage: Option<&casc_core::Storage>,
    labels: Vec<Value>,
) -> Result<(), String> {
    let mut ui = read_local_or_casc_json(mpq, storage, UI_STRINGS)?;
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for row in label_rows(&ui)? {
        register_label(row, &mut keys, &mut ids);
    }
    for file in json_files(&mpq.join(STRINGS), false)? {
        if file
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("ui.json"))
        {
            continue;
        }
        for row in label_rows(&read_json(&file)?)? {
            register_label(row, &mut keys, &mut ids);
        }
    }
    let rows = ui.as_array_mut().unwrap();
    let before = rows.len();
    for mut row in labels {
        let Some(key) = row["Key"].as_str() else {
            return Err("内置 JCY 词条缺少键名".into());
        };
        if !keys.insert(key.to_owned()) {
            continue;
        }
        let id = row["id"].as_u64().ok_or("内置 JCY 词条缺少 ID")?;
        if !ids.insert(id) {
            let next = ids.range(100_000..).next_back().copied().unwrap_or(99_999) + 1;
            if next > i32::MAX as u64 {
                return Err("没有可用的 JCY 语言词条 ID".into());
            }
            ids.insert(next);
            row["id"] = json!(next);
        }
        rows.push(row);
    }
    if rows.len() != before {
        write_json_layout(mpq, UI_STRINGS, &ui)?;
    }
    Ok(())
}

fn remove_replaced_panels(mpq: &Path) -> Result<(), String> {
    let obsolete = [
        "D2RHubInventoryCubehd.json",
        "D2RHubInventoryCubeClosehd.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain((1..=11).map(|page| format!("D2RHubStashPage{page}hd.json")));
    for file in obsolete {
        let path = mpq.join(LAYOUTS).join(file);
        if path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("清理旧布局 {} 失败：{e}", path.display()))?;
        }
    }
    Ok(())
}

fn validate_layouts(mpq: &Path) -> Result<(), String> {
    let mut bag = read_layout(mpq, BAG)?;
    let mut bank = read_layout(mpq, BANK)?;
    let mut cube = read_layout(mpq, CUBE)?;
    let close = read_layout(mpq, CUBE_CLOSE)?;
    if find_layout_node_mut(&mut bag, "OpenJcyMiniCube")
        .is_none_or(|node| node["fields"]["message"] != "PanelManager:OpenPanel:JcyMiniCube")
        || find_layout_node_mut(&mut cube, "grid")
            .is_none_or(|node| node["fields"]["cellCount"] != json!({"x":3,"y":4}))
        || find_layout_node_mut(&mut cube, "convert").is_none_or(|node| {
            node["fields"]["onClickMessage"] != "HoradricCubePanelMessage:Convert"
        })
        || cube["fields"]["rect"] != json!({"x":-1322,"y":-138})
        || close["name"] != "JcyMiniCubeClose"
    {
        return Err("JCY 背包与仓库缺少原内嵌方盒布局".into());
    }
    let count = page_count(&bank);
    if !(9..=11).contains(&count) {
        return Err("JCY 仓库分页不完整".into());
    }
    for page in 1..=count {
        let name = format!("JcyStashPage{page}");
        if read_layout(mpq, &format!("{name}hd.json"))?["name"] != name {
            return Err(format!("JCY 仓库第 {page} 页直达无效"));
        }
    }
    for layout in [&mut bag, &mut bank, &mut cube] {
        let mut missing = None;
        replace_strings(layout, &mut |text| {
            if text.starts_with(SPRITE_PREFIX) {
                for suffix in [".sprite", ".lowend.sprite"] {
                    if !mpq
                        .join(UI_SPRITES)
                        .join(format!("{text}{suffix}"))
                        .is_file()
                    {
                        missing = Some(format!("{text}{suffix}"));
                    }
                }
            }
        });
        if let Some(path) = missing {
            return Err(format!("JCY 背包与仓库缺少贴图：{path}"));
        }
    }
    Ok(())
}

pub(super) fn prepare(
    mpq: &Path,
    storage: Option<&casc_core::Storage>,
    requested: bool,
    inherited: Option<&ModFeatureGroup>,
) -> Result<Option<(ModFeatureGroup, AudioModCompatibility)>, String> {
    if let Some(group) = inherited {
        validate_source(group)?;
        if group.recipe_version == RECIPE {
            check_capacity(mpq)?;
            validate_layouts(mpq)?;
            return Ok(Some((
                feature_group(true),
                AudioModCompatibility {
                    target: "JCY 背包与仓库".into(),
                    action: "reuse_verified_feature_group".into(),
                    detail: "沿用已校验的 JCY 背包、仓库和内嵌方盒。".into(),
                },
            )));
        }
    } else if !requested {
        return Ok(None);
    }
    check_capacity(mpq)?;
    let (preset, sprites) = read_preset()?;
    let count = if mpq.join(LAYOUTS).join(BANK).is_file() {
        page_count(&read_layout(mpq, BANK)?).clamp(9, 11)
    } else {
        9
    };
    let Preset {
        mut bag,
        bank,
        bank_nine,
        mut cube,
        close,
        pages,
        styles,
        labels,
        sprites: assets,
    } = preset;
    let mut bank = if count == 9 { bank_nine } else { bank };
    adjust_page_count(&mut bank, count)?;

    remove_old_layouts(mpq)?;
    remove_replaced_panels(mpq)?;
    install_sprites(mpq, &assets, &sprites)?;
    install_profile(mpq, storage, styles, &mut bag, &mut bank, &mut cube)?;
    install_labels(mpq, storage, labels)?;
    if mpq.join(LAYOUTS).join(ALTERNATE_BAG).is_file() {
        let mut old = read_layout(mpq, ALTERNATE_BAG)?;
        let mut alternate = bag.clone();
        alternate["name"] = old["name"].clone();
        if let Some(priority) = old["fields"].get("priority") {
            alternate["fields"]["priority"] = priority.clone();
        }
        if let (Some(previous), Some(widget)) = (
            find_layout_node_mut(&mut old, "close"),
            find_layout_node_mut(&mut alternate, "close"),
        ) {
            if let Some(fields) = previous.get("fields") {
                widget["fields"] = fields.clone();
            }
        }
        write_layout(mpq, ALTERNATE_BAG, &alternate)?;
    }
    for (file, layout) in [
        (BAG, &bag),
        (BANK, &bank),
        (CUBE, &cube),
        (CUBE_CLOSE, &close),
    ] {
        write_layout(mpq, file, layout)?;
    }
    for (index, page) in pages.iter().take(count).enumerate() {
        write_layout(mpq, &format!("JcyStashPage{}hd.json", index + 1), page)?;
    }
    validate_layouts(mpq)?;
    Ok(Some((
        feature_group(false),
        AudioModCompatibility {
            target: "JCY 背包与仓库".into(),
            action: "replace_inventory_with_reference".into(),
            detail: format!(
                "应用内置参考布局和贴图，背包保留 3×4 方盒及转化按钮，仓库保留 {count} 页直达。"
            ),
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_grid_is_reported_without_changing_storage_data() {
        let table = TsvTable::parse(INVENTORY, "class\tgridX\tgridY\nAmazon2\t10\t8").unwrap();
        assert!(check_table_capacity(&table)
            .unwrap_err()
            .contains("Amazon2"));
        assert_eq!(table.rows[0][2], "8");
    }

    #[test]
    fn old_cube_removal_keeps_unrelated_hud_buttons() {
        let keep = json!({"name":"RoomTools","fields":{"message":"PanelManager:OpenPanel:D2RHubRoomTools"}});
        let mut hud = json!({"children":[
            {"name":"Old","fields":{"message":"PanelManager:OpenPanel:D2RHubInventoryCube"}},
            {"name":"MYMOD","fields":{"message":"PanelManager:TogglePanel:HoradricCubeLayoutMiniCenter"}},
            keep.clone()
        ]});
        assert!(disable_old_cube(&mut hud, true));
        assert_eq!(hud["children"], json!([keep]));
    }

    #[test]
    fn bundled_reference_has_both_page_counts_and_the_embedded_cube() {
        if !preset_available() {
            return;
        }
        let (preset, sprites) = read_preset().unwrap();
        assert_eq!(page_count(&preset.bank), 11);
        assert_eq!(page_count(&preset.bank_nine), 9);
        assert_eq!(preset.cube["fields"]["rect"], json!({"x":-1322,"y":-138}));
        assert_eq!(preset.pages.len(), 11);
        assert_eq!(
            preset.sprites.iter().map(|(_, len)| len).sum::<usize>(),
            sprites.len()
        );
    }
}
