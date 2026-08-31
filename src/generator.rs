use crate::audio::{decode_flac, encode_flac, resample_interleaved_i32};
use d2r_audio_protocol::catalog::{
    marker_sort_key, AreaCatalogEntry, AreaCatalogFile, LocationKind, TelemetryMarker,
    AREA_CATALOG_FILE_NAME, MAX_AREA_ID, RUNE_COUNT,
};
use d2r_audio_protocol::item_catalog::{
    catalog_file as build_item_catalog_file, default_tracked_categories,
    normalize_tracked_categories, selected_item_definitions, ItemCatalogEntry,
    SupportedItemDefinition, CATEGORY_RUNES, ITEM_CATALOG_FILE_NAME,
};
use d2r_audio_protocol::protocol::{
    detect_markers, embed_marker, interleaved_i32_to_mono, marker_frames, MarkerConfig,
    MARKER_OFFSET_SECONDS, MIN_SAMPLE_RATE, PACKET_GAP_SECONDS, PROTOCOL_VERSION,
};
use d2r_audio_protocol::rune_data;
use encoding_rs::{Encoding, GB18030, UTF_16BE, UTF_16LE, WINDOWS_1252};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MINIMAL_MOD_NAME: &str = "D2RAudioTelemetry";
const AUDIO_MOD_SUFFIX: &str = "AudioTelemetry";
const COUNTESS_AREA_IDS: [u32; 8] = [1, 6, 20, 21, 22, 23, 24, 25];
const TERROR_PROBE_MARKER_AREA_ID: u32 = MAX_AREA_ID;
const TERROR_PROBE_SD_SOUND: &str = "audio_telemetry_tz_probe_sd";
const TERROR_PROBE_HD_SOUND: &str = "audio_telemetry_tz_probe_hd";
const TERROR_PROBE_RELATIVE_PATH: &str = "audio_telemetry\\terror\\tz_probe.flac";
const TERROR_MARKER_GAIN_DB: f32 = -18.0;
pub const AUDIO_MOD_RECIPE_VERSION: u32 = 6;
const TERROR_IMMEDIATE_ENTRY_CAPABILITY: &str = "terror_zone_immediate_entry_marker_v1";
const IN_GAME_ROOM_TOOLS_CAPABILITY: &str = "in_game_room_tools_v4";
const UI_LAYOUTS_DIRECTORY: &str = "data/global/ui/layouts";
const HUD_WARNINGS_LAYOUT: &str = "data/global/ui/layouts/HudWarningshd.json";
const ROOM_TOOLBAR_PANEL: &str = "D2RHubRoomToolbar";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioModBuildMode {
    Minimal,
    #[default]
    Augment,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioAreaCoverage {
    #[default]
    CountessRoute,
    AllAreas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildAudioModRequest {
    #[serde(default)]
    pub build_mode: AudioModBuildMode,
    #[serde(default)]
    pub source_directory: Option<String>,
    #[serde(default)]
    pub game_directory: Option<String>,
    #[serde(default)]
    pub area_coverage: AudioAreaCoverage,
    /// Marker categories included in the generated Mod.
    #[serde(default = "default_tracked_categories")]
    pub tracked_categories: Vec<String>,
    pub output_directory: Option<String>,
    /// Optional output Mod name. It becomes both the outer directory and `.mpq` directory name.
    #[serde(default)]
    pub mod_name: Option<String>,
    pub sound_environment_file: Option<String>,
    pub gain_db: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioModAsset {
    pub marker: TelemetryMarker,
    pub label: String,
    pub sound: String,
    pub relative_path: String,
    pub source_audio: Option<String>,
    pub preserved_source_audio: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildAudioModReport {
    /// Stable machine-readable manifest identity. Receivers should validate this before trusting
    /// the generated catalogs.
    pub manifest_format: String,
    pub producer: String,
    pub producer_version: String,
    /// Incremented only when an existing generated Mod must be rebuilt to gain required assets.
    pub recipe_version: u32,
    /// Additive feature identifiers help newer consumers explain why a rebuild is recommended.
    pub capabilities: Vec<String>,
    pub generated_at_unix: u64,
    pub protocol_version: u8,
    pub build_mode: AudioModBuildMode,
    pub area_coverage: AudioAreaCoverage,
    pub mod_name: String,
    pub mod_directory: String,
    pub mpq_directory: String,
    pub source_excel_directory: String,
    pub source_mod_copied: bool,
    /// Best-effort source Mod hint for future guided rebuilds. Older manifests omit this field.
    pub source_mod_name: Option<String>,
    pub sound_environment_source: String,
    pub launch_arguments: String,
    pub rune_assets: Vec<AudioModAsset>,
    pub item_assets: Vec<AudioModAsset>,
    pub area_assets: Vec<AudioModAsset>,
    pub terror_assets: Vec<AudioModAsset>,
    pub frontend_assets: Vec<AudioModAsset>,
    pub area_catalog: Vec<AreaCatalogEntry>,
    pub compatibility: Vec<AudioModCompatibility>,
    pub notes: Vec<String>,
}

fn inferred_source_mod_name(source_directory: Option<&str>) -> Option<String> {
    let path = Path::new(source_directory?.trim());
    let leaf = path.file_name()?.to_string_lossy();
    if leaf.to_ascii_lowercase().ends_with(".mpq") {
        path.file_stem()
            .map(|value| value.to_string_lossy().into_owned())
    } else {
        Some(leaf.into_owned())
    }
    .filter(|value| !value.trim().is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildProgress {
    pub phase: String,
    pub percent: u8,
    pub message: String,
}

impl BuildProgress {
    fn new(phase: &str, percent: u8, message: impl Into<String>) -> Self {
        Self {
            phase: phase.to_string(),
            percent: percent.min(100),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioModCompatibility {
    pub target: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
struct TsvTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TsvTable {
    fn parse(name: &str, text: &str) -> Result<Self, String> {
        let normalized = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut lines = normalized.lines();
        let header = lines
            .next()
            .ok_or_else(|| format!("{name} 是空文件"))?
            .trim_end_matches('\r');
        let headers = header.split('\t').map(str::to_string).collect::<Vec<_>>();
        if headers.is_empty() {
            return Err(format!("{name} 缺少表头"));
        }
        let rows = lines
            .filter_map(|line| {
                let line = line.trim_end_matches('\r');
                (!line.is_empty()).then(|| {
                    let mut row = line.split('\t').map(str::to_string).collect::<Vec<_>>();
                    row.resize(headers.len(), String::new());
                    row.truncate(headers.len());
                    row
                })
            })
            .collect();
        Ok(Self { headers, rows })
    }

    fn column(&self, name: &str) -> Result<usize, String> {
        self.headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("数据表缺少列“{name}”"))
    }

    fn set(&self, row: &mut [String], name: &str, value: impl Into<String>) -> Result<(), String> {
        row[self.column(name)?] = value.into();
        Ok(())
    }

    #[cfg(test)]
    fn get<'a>(&self, row: &'a [String], name: &str) -> Option<&'a str> {
        self.column(name)
            .ok()
            .and_then(|index| row.get(index))
            .map(String::as_str)
    }

    fn row_by(&self, column: &str, value: &str) -> Result<Vec<String>, String> {
        let index = self.column(column)?;
        self.rows
            .iter()
            .find(|row| row[index].eq_ignore_ascii_case(value))
            .cloned()
            .ok_or_else(|| format!("数据表中找不到 {column}={value} 的模板行"))
    }

    fn max_number(&self, column: &str) -> Result<u32, String> {
        let index = self.column(column)?;
        self.rows
            .iter()
            .filter_map(|row| row[index].trim().parse::<u32>().ok())
            .max()
            .ok_or_else(|| format!("列“{column}”中没有有效数字"))
    }

    fn to_text(&self) -> String {
        let mut output = String::new();
        output.push_str(&self.headers.join("\t"));
        output.push_str("\r\n");
        for row in &self.rows {
            output.push_str(&row.join("\t"));
            output.push_str("\r\n");
        }
        output
    }
}

#[derive(Debug)]
struct SourceLayout {
    excel: PathBuf,
    mpq: Option<PathBuf>,
}

#[derive(Debug)]
struct ResolvedTextFile {
    text: String,
    source: String,
    from_source_mod: bool,
}

#[derive(Debug, Clone)]
struct SoundDefinition {
    marker: TelemetryMarker,
    group: SoundAssetGroup,
    sound: String,
    relative_path: String,
    source_filename: Option<String>,
    output_root: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundAssetGroup {
    Rune,
    Item,
    Area,
    Terror,
    Frontend,
}

#[derive(Debug, Clone)]
struct RuneStatePlan {
    rune_number: u32,
    original_audio_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ItemStatePlan {
    entry: ItemCatalogEntry,
    original_audio_id: Option<String>,
}

#[derive(Debug)]
struct ResolvedItemEntityAsset {
    document: serde_json::Value,
    source: String,
    asset: String,
    used_baseline_mapping: bool,
    preferred_error: Option<String>,
}

#[derive(Debug)]
struct ResolvedAudioSource {
    // Some Mods intentionally override a game sound with an empty FLAC file.
    // Keep that as silence instead of falling back to the game's original audio.
    path: Option<PathBuf>,
    label: String,
}

struct StagingDirectory {
    path: PathBuf,
    parent: PathBuf,
    prefix: String,
    committed: bool,
}

impl StagingDirectory {
    fn create(output_parent: &Path, mod_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(output_parent).map_err(|error| {
            format!("创建 Mod 输出目录失败 {}: {error}", output_parent.display())
        })?;
        let parent = std::fs::canonicalize(output_parent).map_err(|error| {
            format!("解析 Mod 输出目录失败 {}: {error}", output_parent.display())
        })?;
        let prefix = format!(".{mod_name}.building-");
        let path = parent.join(format!("{prefix}{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)
            .map_err(|error| format!("创建临时 Mod 目录失败 {}: {error}", path.display()))?;
        Ok(Self {
            path,
            parent,
            prefix,
            committed: false,
        })
    }

    fn commit(mut self, target: &Path) -> Result<(), String> {
        if target.exists() {
            return Err(format!("输出 Mod 已存在，拒绝覆盖: {}", target.display()));
        }
        std::fs::rename(&self.path, target).map_err(|error| {
            format!(
                "提交生成的 Mod 失败 {} -> {}: {error}",
                self.path.display(),
                target.display()
            )
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if self.committed || self.path.parent() != Some(self.parent.as_path()) {
            return;
        }
        if !self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with(&self.prefix))
        {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn has_any_excel_file(path: &Path) -> bool {
    ["misc.txt", "sounds.txt", "levels.txt", "soundenviron.txt"]
        .iter()
        .any(|name| path.join(name).is_file())
}

fn is_mpq_directory(path: &Path) -> bool {
    path.is_dir()
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("mpq"))
}

fn layout_from_mpq(mpq: PathBuf) -> SourceLayout {
    SourceLayout {
        excel: mpq.join("data/global/excel"),
        mpq: Some(mpq),
    }
}

fn find_source_layout(source: &Path) -> Result<SourceLayout, String> {
    if !source.is_dir() {
        return Err(format!(
            "源 Mod 目录不存在或不是文件夹: {}",
            source.display()
        ));
    }
    if has_any_excel_file(source)
        || source
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("excel"))
    {
        let mpq = source.ancestors().find(|ancestor| {
            ancestor
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("mpq"))
        });
        return Ok(SourceLayout {
            excel: source.to_path_buf(),
            mpq: mpq.map(Path::to_path_buf),
        });
    }
    if is_mpq_directory(source) || source.join("data/global/excel").is_dir() {
        return Ok(layout_from_mpq(source.to_path_buf()));
    }

    let mut mpq_directories = std::fs::read_dir(source)
        .map_err(|error| format!("读取源目录失败 {}: {error}", source.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_mpq_directory(path))
        .collect::<Vec<_>>();
    mpq_directories.sort();
    if let Some(source_name) = source.file_name().and_then(|value| value.to_str()) {
        if let Some(matching) = mpq_directories.iter().find(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(source_name))
        }) {
            return Ok(layout_from_mpq(matching.clone()));
        }
    }
    if mpq_directories.len() == 1 {
        return Ok(layout_from_mpq(mpq_directories.remove(0)));
    }
    if mpq_directories.len() > 1 {
        return Err(format!(
            "{} 中存在多个 .mpq 文件夹，请直接选择需要加工的 .mpq 文件夹",
            source.display()
        ));
    }
    Err(format!(
        "在 {} 中找不到可加工的 .mpq 文件夹",
        source.display()
    ))
}

fn non_empty_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn is_game_storage_root(path: &Path) -> bool {
    path.join(".build.info").is_file() && path.join("Data").is_dir()
}

fn find_game_storage_root(
    explicit_game_directory: Option<&Path>,
    source: Option<&Path>,
    output_parent: &Path,
) -> Option<PathBuf> {
    explicit_game_directory
        .into_iter()
        .chain(source)
        .chain(std::iter::once(output_parent))
        .flat_map(Path::ancestors)
        .find(|candidate| is_game_storage_root(candidate))
        .map(Path::to_path_buf)
}

fn sanitize_mod_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        MINIMAL_MOD_NAME.to_string()
    } else {
        sanitized
    }
}

fn default_mod_name(mode: AudioModBuildMode, layout: Option<&SourceLayout>) -> String {
    match mode {
        AudioModBuildMode::Minimal => MINIMAL_MOD_NAME.to_string(),
        AudioModBuildMode::Augment => layout
            .and_then(|layout| layout.mpq.as_deref())
            .and_then(Path::file_stem)
            .and_then(|value| value.to_str())
            .map(|value| sanitize_mod_name(&format!("{value}-{AUDIO_MOD_SUFFIX}")))
            .unwrap_or_else(|| format!("Mod-{AUDIO_MOD_SUFFIX}")),
    }
}

fn requested_mod_name(
    value: Option<&str>,
    mode: AudioModBuildMode,
    layout: Option<&SourceLayout>,
) -> Result<String, String> {
    let Some(value) = value else {
        return Ok(default_mod_name(mode, layout));
    };
    let value = value.trim();
    if value.is_empty() {
        return Err("--name 不能为空".to_string());
    }
    if value.len() > 128 {
        return Err("--name 不能超过 128 个 ASCII 字符".to_string());
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("--name 仅允许 ASCII 字母、数字、连字符 (-) 和下划线 (_)".to_string());
    }
    let uppercase = value.to_ascii_uppercase();
    let reserved = matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (uppercase.len() == 4
            && (uppercase.starts_with("COM") || uppercase.starts_with("LPT"))
            && matches!(uppercase.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return Err(format!("--name 不能使用 Windows 保留名称: {value}"));
    }
    Ok(value.to_string())
}

fn available_mod_name(output_parent: &Path, base_name: &str) -> String {
    if !output_parent.join(base_name).exists() {
        return base_name.to_string();
    }
    (2..10_000)
        .map(|suffix| format!("{base_name}-{suffix}"))
        .find(|candidate| !output_parent.join(candidate).exists())
        .unwrap_or_else(|| format!("{base_name}-{}", uuid::Uuid::new_v4().simple()))
}

fn casc_path(internal_path: &str) -> String {
    let normalized = internal_path.replace('/', "\\");
    if normalized.starts_with("data:") {
        normalized
    } else {
        format!("data:{normalized}")
    }
}

fn read_casc_utf8(storage: &casc_core::Storage, internal_path: &str) -> Result<String, String> {
    let path = casc_path(internal_path);
    let bytes = storage
        .read(&path)
        .map_err(|error| format!("从 D2R CASC 读取失败 {path}: {error}"))?;
    String::from_utf8(bytes).map_err(|error| format!("D2R CASC 文件不是 UTF-8 {path}: {error}"))
}

fn extract_casc_file(
    storage: &casc_core::Storage,
    internal_path: &str,
    target: &Path,
) -> Result<(), String> {
    let path = casc_path(internal_path);
    let bytes = storage
        .read(&path)
        .map_err(|error| format!("从 D2R CASC 提取失败 {path}: {error}"))?;
    write_file(target, bytes)
}

fn extract_minimal_baseline(
    storage: &casc_core::Storage,
    mpq_directory: &Path,
) -> Result<SourceLayout, String> {
    for name in ["misc.txt", "sounds.txt", "levels.txt", "soundenviron.txt"] {
        let internal = format!("data/global/excel/{name}");
        extract_casc_file(
            storage,
            &internal,
            &mpq_directory.join(format!("data/global/excel/{name}")),
        )?;
    }
    extract_casc_file(
        storage,
        "data/hd/items/items.json",
        &mpq_directory.join("data/hd/items/items.json"),
    )?;
    for rune_number in 1..=RUNE_COUNT {
        let rune_name = rune_data::RUNE_NAMES_EN[(rune_number - 1) as usize].to_ascii_lowercase();
        let relative = format!("data/hd/items/misc/rune/{rune_name}_rune.json");
        extract_casc_file(storage, &relative, &mpq_directory.join(&relative))?;
    }
    Ok(SourceLayout {
        excel: mpq_directory.join("data/global/excel"),
        mpq: None,
    })
}

fn read_utf8(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("读取失败 {}: {error}", path.display()))
}

fn decode_without_replacement(encoding: &'static Encoding, bytes: &[u8]) -> Option<String> {
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| text.into_owned())
}

fn likely_utf16_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let sample_len = bytes.len().min(1024) & !1;
    if sample_len < 8 {
        return None;
    }
    let pairs = sample_len / 2;
    let even_zeroes = bytes[..sample_len]
        .iter()
        .step_by(2)
        .filter(|&&byte| byte == 0)
        .count();
    let odd_zeroes = bytes[1..sample_len]
        .iter()
        .step_by(2)
        .filter(|&&byte| byte == 0)
        .count();
    if odd_zeroes * 3 >= pairs && even_zeroes * 20 <= pairs {
        Some(UTF_16LE)
    } else if even_zeroes * 3 >= pairs && odd_zeroes * 20 <= pairs {
        Some(UTF_16BE)
    } else {
        None
    }
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(
            character as u32,
            0x3400..=0x9fff | 0x20000..=0x2fa1f
        )
    })
}

fn decode_gb18030_source_mod(bytes: &[u8]) -> Option<String> {
    // Legacy Diablo tables may mix GBK text with the raw 0xFF byte used by
    // the game's `ÿc` color controls. 0xFF is not a valid GBK/GB18030 byte,
    // so decode the valid spans and restore that established control byte.
    let mut decoded = String::new();
    let mut start = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte != 0xff {
            continue;
        }
        decoded.push_str(&decode_without_replacement(GB18030, &bytes[start..index])?);
        decoded.push('\u{00ff}');
        start = index + 1;
    }
    decoded.push_str(&decode_without_replacement(GB18030, &bytes[start..])?);
    contains_cjk(&decoded).then_some(decoded)
}

/// Source Mods are commonly saved by spreadsheet editors as UTF-8, UTF-16,
/// GBK/GB18030, or the current Western "ANSI" encoding. Decode those inputs
/// without modifying the source; generated tables are always written as UTF-8.
fn decode_excel_text(bytes: &[u8]) -> Result<String, String> {
    let decoded = if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| format!("UTF-8 BOM 后的数据无效: {error}"))?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        decode_without_replacement(UTF_16LE, bytes)
            .ok_or_else(|| "UTF-16 LE 文本包含无效字符".to_string())?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        decode_without_replacement(UTF_16BE, bytes)
            .ok_or_else(|| "UTF-16 BE 文本包含无效字符".to_string())?
    } else if let Some(encoding) = likely_utf16_encoding(bytes) {
        decode_without_replacement(encoding, bytes)
            .ok_or_else(|| format!("疑似 {} 文本，但内容无效", encoding.name()))?
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        text.to_owned()
    } else if let Some(text) = decode_gb18030_source_mod(bytes) {
        text
    } else {
        decode_without_replacement(WINDOWS_1252, bytes)
            .ok_or_else(|| "不是受支持的 UTF-8、UTF-16、GBK 或 ANSI 文本".to_string())?
    };
    if decoded.contains('\0') {
        return Err("文本包含 NUL 字节，疑似不是有效的数据表".to_string());
    }
    Ok(decoded
        .strip_prefix('\u{feff}')
        .unwrap_or(&decoded)
        .to_string())
}

fn read_excel_text(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("读取失败 {}: {error}", path.display()))?;
    decode_excel_text(&bytes).map_err(|error| format!("读取文本表失败 {}: {error}", path.display()))
}

fn read_text_with_fallback<F>(
    local_path: &Path,
    fallback_source: String,
    fallback: F,
) -> Result<ResolvedTextFile, String>
where
    F: FnOnce() -> Result<String, String>,
{
    if local_path.is_file() {
        return Ok(ResolvedTextFile {
            text: read_excel_text(local_path)?,
            source: local_path.to_string_lossy().into_owned(),
            from_source_mod: true,
        });
    }
    Ok(ResolvedTextFile {
        text: fallback()?,
        source: fallback_source,
        from_source_mod: false,
    })
}

fn read_excel_with_game_fallback(
    layout: &SourceLayout,
    storage: Option<&casc_core::Storage>,
    game_root: Option<&Path>,
    name: &str,
) -> Result<ResolvedTextFile, String> {
    let local_path = layout.excel.join(name);
    let internal_path = format!("data/global/excel/{name}");
    let fallback_source = format!(
        "CASC:{}:{internal_path}",
        game_root.unwrap_or_else(|| Path::new("?")).display()
    );
    read_text_with_fallback(&local_path, fallback_source, || {
        let storage = storage.ok_or_else(|| {
            format!(
                "源 Mod 缺少 data/global/excel/{name}，且没有可用的 D2R 游戏数据；请在本机游戏目录中运行或显式提供 --game"
            )
        })?;
        read_casc_utf8(storage, &internal_path)
    })
}

fn set_if_present(table: &TsvTable, row: &mut [String], name: &str, value: &str) {
    if let Ok(index) = table.column(name) {
        row[index] = value.to_string();
    }
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("创建目录失败 {}: {error}", target.display()))?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| format!("读取源 Mod 失败 {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("读取源 Mod 项失败: {error}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取源 Mod 项类型失败 {}: {error}", source_path.display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "源 Mod 含符号链接/目录联接，拒绝递归复制: {}",
                source_path.display()
            ));
        }
        if file_type.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path).map_err(|error| {
                format!(
                    "复制源 Mod 资源失败 {} -> {}: {error}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn write_file(path: &Path, content: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录失败 {}: {error}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|error| format!("写入失败 {}: {error}", path.display()))
}

fn room_toolbar_layout() -> serde_json::Value {
    serde_json::json!({
        "type": "TooltipsPanel",
        "name": ROOM_TOOLBAR_PANEL,
        "fields": {
            "priority": 5,
            "anchor": { "x": 1.0 },
            "rect": { "scale": 1.0 }
        },
        "children": [
            {
                "type": "ButtonWidget",
                "name": "D2RHubNextGame",
                "fields": {
                    "anchor": { "x": 1.0 },
                    "rect": { "x": -1510, "y": 12, "scale": 0.36 },
                    "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonLarge",
                    "textString": "下一局",
                    "tooltipString": "首次点击只打开确认条；再次确认才会离开当前房间，4 秒后自动取消。",
                    "onClickMessage": "PanelManager:TogglePanel:D2RHubQuickRecreateConfirm",
                    "text/style": "$StyleFEButtonText",
                    "pointSize": 53,
                    "textColor": "$FontColorOrange",
                    "hoveredFrame": 3,
                    "disabledFrame": 2,
                    "disabledTint": { "a": 1.0 },
                    "sound": "cursor_launch_game_hd"
                }
            },
            {
                "type": "ButtonWidget",
                "name": "D2RHubCreateGame",
                "fields": {
                    "anchor": { "x": 1.0 },
                    "rect": { "x": -1160, "y": 12, "scale": 0.36 },
                    "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonLarge",
                    "textString": "创建房间",
                    "tooltipString": "打开创建房间面板；再次点击或按 Esc 关闭",
                    "onClickMessage": "PanelManager:OpenPanel:D2RHubOpenCreateGame",
                    "text/style": "$StyleFEButtonText",
                    "pointSize": 53,
                    "textColor": "$FontColorLightYellow",
                    "hoveredFrame": 3,
                    "disabledFrame": 2,
                    "disabledTint": { "a": 1.0 },
                    "sound": "cursor_select"
                }
            },
            {
                "type": "ButtonWidget",
                "name": "D2RHubJoinGame",
                "fields": {
                    "anchor": { "x": 1.0 },
                    "rect": { "x": -810, "y": 12, "scale": 0.36 },
                    "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonLarge",
                    "textString": "加入房间",
                    "tooltipString": "打开加入房间面板；再次点击或按 Esc 关闭",
                    "onClickMessage": "PanelManager:OpenPanel:D2RHubOpenJoinGame",
                    "text/style": "$StyleFEButtonText",
                    "pointSize": 53,
                    "textColor": "$FontColorLightYellow",
                    "hoveredFrame": 3,
                    "disabledFrame": 2,
                    "disabledTint": { "a": 1.0 },
                    "sound": "cursor_select"
                }
            }
        ]
    })
}

fn quick_recreate_confirmation_layout() -> serde_json::Value {
    serde_json::json!({
        "type": "Panel",
        "name": "D2RHubQuickRecreateConfirm",
        "fields": {
            "priority": 6,
            "anchor": { "x": 1.0 },
            "rect": { "scale": 1.0 },
            "isDismissable": true,
            "acceptsEscKeyEverywhere": true
        },
        "children": [
            {
                "type": "ButtonWidget",
                "name": "D2RHubConfirmNextGame",
                "fields": {
                    "anchor": { "x": 1.0 },
                    "rect": { "x": -1510, "y": 105, "scale": 0.36 },
                    "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonLarge",
                    "textString": "确认换房",
                    "tooltipString": "立即离开当前房间并用当前角色开始下一局",
                    "onClickMessage": "PanelManager:OpenPanel:D2RHubQuickRecreate",
                    "text/style": "$StyleFEButtonText",
                    "pointSize": 53,
                    "textColor": "$FontColorOrange",
                    "hoveredFrame": 3,
                    "disabledFrame": 2,
                    "disabledTint": { "a": 1.0 },
                    "sound": "cursor_launch_game_hd"
                }
            },
            {
                "type": "ButtonWidget",
                "name": "D2RHubCancelNextGame",
                "fields": {
                    "anchor": { "x": 1.0 },
                    "rect": { "x": -1160, "y": 105, "scale": 0.36 },
                    "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonLarge",
                    "textString": "取消",
                    "onClickMessage": "PanelManager:ClosePanel:D2RHubQuickRecreateConfirm",
                    "text/style": "$StyleFEButtonText",
                    "pointSize": 53,
                    "textColor": "$FontColorLightYellow",
                    "hoveredFrame": 3,
                    "disabledFrame": 2,
                    "disabledTint": { "a": 1.0 },
                    "sound": "cursor_close_window_hd"
                }
            },
            {
                "type": "TimerWidget",
                "name": "D2RHubRecreateConfirmTimeout",
                "fields": {
                    "time": 4.0,
                    "message": "PanelManager:ClosePanel:D2RHubQuickRecreateConfirm"
                }
            }
        ]
    })
}

fn quick_recreate_layout() -> serde_json::Value {
    serde_json::json!({
        "type": "MainMenuHDPanel",
        "name": "D2RHubQuickRecreate",
        "fields": {
            "rect": { "x": -9999, "y": -9999, "scale": 0.01 }
        },
        "children": [
            {
                "type": "TimerWidget",
                "name": "D2RHubQuickRecreateCloseConfirm",
                "fields": { "time": 0.005, "message": "PanelManager:ClosePanel:D2RHubQuickRecreateConfirm" }
            },
            {
                "type": "TimerWidget",
                "name": "D2RHubQuickRecreateAction",
                "fields": { "time": 0.01, "message": "CharacterSelect:LoadCharacter:2" }
            },
            {
                "type": "TimerWidget",
                "name": "D2RHubQuickRecreateClose",
                "fields": { "time": 0.02, "message": "PanelManager:ClosePanel:D2RHubQuickRecreate" }
            }
        ]
    })
}

fn room_panel_opener_layout(create: bool) -> serde_json::Value {
    let (name, opposite_panel, target_panel) = if create {
        ("D2RHubOpenCreateGame", "JoinGamePanel", "CreateGamePanel")
    } else {
        ("D2RHubOpenJoinGame", "CreateGamePanel", "JoinGamePanel")
    };
    serde_json::json!({
        "type": "Panel",
        "name": name,
        "children": [
            {
                "type": "TimerWidget",
                "name": "D2RHubOpenRoomPanel",
                "fields": {
                    "time": 0.1,
                    "message": format!("PanelManager:TogglePanel:{target_panel}")
                }
            },
            {
                "type": "TimerWidget",
                "name": "D2RHubCloseOppositeRoomPanel",
                "fields": {
                    "time": 0.1,
                    "message": format!("PanelManager:ClosePanel:{opposite_panel}")
                }
            },
            {
                "type": "TimerWidget",
                "name": "D2RHubCloseRoomPanelOpener",
                "fields": {
                    "time": 0.1,
                    "message": format!("PanelManager:ClosePanel:{name}")
                }
            }
        ]
    })
}

fn read_local_or_casc_json(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    relative_path: &str,
) -> Result<serde_json::Value, String> {
    let local_path = mpq_directory.join(relative_path);
    let text = if local_path.is_file() {
        read_utf8(&local_path)?
    } else {
        let storage = storage.ok_or_else(|| {
            format!("源 Mod 缺少 {relative_path}，且没有可用的 D2R 游戏数据用于补齐局内房间工具")
        })?;
        read_casc_utf8(storage, relative_path)?
    };
    parse_json_value(&text).map_err(|error| format!("解析 {relative_path} 失败: {error}"))
}

fn write_json_layout(
    mpq_directory: &Path,
    relative_path: &str,
    document: &serde_json::Value,
) -> Result<(), String> {
    write_file(
        &mpq_directory.join(relative_path),
        serde_json::to_vec_pretty(document)
            .map_err(|error| format!("序列化 {relative_path} 失败: {error}"))?,
    )
}

fn find_layout_node_mut<'a>(
    document: &'a mut serde_json::Value,
    name: &str,
) -> Option<&'a mut serde_json::Value> {
    if document.get("name").and_then(serde_json::Value::as_str) == Some(name) {
        return Some(document);
    }
    document
        .get_mut("children")
        .and_then(serde_json::Value::as_array_mut)?
        .iter_mut()
        .find_map(|child| find_layout_node_mut(child, name))
}

#[cfg(test)]
fn find_layout_node<'a>(
    document: &'a serde_json::Value,
    name: &str,
) -> Option<&'a serde_json::Value> {
    if document.get("name").and_then(serde_json::Value::as_str) == Some(name) {
        return Some(document);
    }
    document
        .get("children")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find_map(|child| find_layout_node(child, name))
}

fn patch_room_form_layout(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    relative_path: &str,
    primary_input_name: &str,
) -> Result<(), String> {
    let mut document = read_local_or_casc_json(mpq_directory, storage, relative_path)?;
    let fields = document
        .as_object_mut()
        .ok_or_else(|| format!("{relative_path} 顶层必须是 JSON 对象"))?
        .entry("fields")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{relative_path}.fields 必须是 JSON 对象"))?;
    fields.insert("priority".to_string(), serde_json::json!(2));
    fields.remove("defaultWidget");
    fields.insert("isDismissable".to_string(), serde_json::json!(true));
    fields.insert(
        "acceptsEscKeyEverywhere".to_string(),
        serde_json::json!(true),
    );

    let (panel_name, input_names): (&str, &[&str]) = if primary_input_name == "NameInput" {
        ("JoinGamePanel", &["NameInput", "PasswordInput"])
    } else {
        (
            "CreateGamePanel",
            &["GameNameInput", "PasswordInput", "DescriptionInput"],
        )
    };
    for &input_name in input_names {
        let input_fields = find_layout_node_mut(&mut document, input_name)
            .and_then(|node| node.get_mut("fields"))
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| format!("{relative_path} 缺少输入框 {input_name}"))?;
        // D2R's in-game chat input uses alwaysAcceptsKeyInput so a focused
        // text box receives letters and digits before the skill-hotkey layer.
        // Keep IME enabled as the native lobby and MDK forms do.
        input_fields.insert("imeEnabled".to_string(), serde_json::json!(true));
        input_fields.insert("alwaysAcceptsKeyInput".to_string(), serde_json::json!(true));
    }
    if primary_input_name == "NameInput" {
        if let Some(join_button_fields) = document
            .get_mut("children")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|children| {
                children.iter_mut().find(|child| {
                    child.get("name").and_then(serde_json::Value::as_str) == Some("JoinButton")
                })
            })
            .and_then(|child| child.get_mut("fields"))
            .and_then(serde_json::Value::as_object_mut)
        {
            let x = join_button_fields
                .get("rect")
                .and_then(|rect| rect.get("x"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default();
            let y = join_button_fields
                .get("rect")
                .and_then(|rect| rect.get("y"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default();
            if x < 0 || y > 1200 {
                join_button_fields.insert(
                    "rect".to_string(),
                    serde_json::json!({
                        "x": 330,
                        "y": 1080
                    }),
                );
                join_button_fields.insert(
                    "filename".to_string(),
                    serde_json::Value::String(
                        "FrontEnd\\HD\\Final\\FrontEnd_ButtonMed".to_string(),
                    ),
                );
            }
        }
    }
    let children = document
        .get_mut("children")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| format!("{relative_path}.children 必须是 JSON 数组"))?;
    children.retain(|child| {
        child.get("name").and_then(serde_json::Value::as_str) != Some("D2RHubCloseRoomForm")
    });
    children.push(serde_json::json!({
        "type": "ButtonWidget",
        "name": "D2RHubCloseRoomForm",
        "fields": {
            "rect": { "x": 820, "y": 1080, "scale": 0.55 },
            "filename": "FrontEnd\\HD\\Final\\FrontEnd_ButtonMed",
            "textString": "@strClose",
            "tooltipString": "@strClose",
            "onClickMessage": format!("PanelManager:ClosePanel:{panel_name}"),
            "fontType": "10ptE",
            "pointSize": "$MediumLargeFontSize",
            "textColor": "$LightButtonTextColor",
            "hoveredFrame": 3,
            "disabledFrame": 2,
            "disabledTint": { "a": 1.0 },
            "sound": "cursor_close_window_hd",
            "acceptsEscKeyEverywhere": true
        }
    }));
    write_json_layout(mpq_directory, relative_path, &document)
}

fn install_in_game_room_tools(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<bool, String> {
    let required_game_layouts = [
        HUD_WARNINGS_LAYOUT,
        "data/global/ui/layouts/creategamepanelhd.json",
        "data/global/ui/layouts/joingamepanelhd.json",
    ];
    if storage.is_none()
        && required_game_layouts
            .iter()
            .any(|relative_path| !mpq_directory.join(relative_path).is_file())
    {
        compatibility.push(AudioModCompatibility {
            target: "局内房间工具".to_string(),
            action: "skip_without_game_layouts".to_string(),
            detail: "源 Mod 未包含完整 HUD/建房/加入布局，且未提供可读取的 D2R 游戏目录；为避免用不完整布局覆盖游戏原界面，本次未追加局内房间工具。".to_string(),
        });
        return Ok(false);
    }
    let mut hud = read_local_or_casc_json(mpq_directory, storage, HUD_WARNINGS_LAYOUT)?;
    let children = hud
        .as_object_mut()
        .ok_or_else(|| format!("{HUD_WARNINGS_LAYOUT} 顶层必须是 JSON 对象"))?
        .entry("children")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("{HUD_WARNINGS_LAYOUT}.children 必须是 JSON 数组"))?;
    children.retain(|child| {
        child
            .pointer("/fields/message")
            .and_then(serde_json::Value::as_str)
            != Some("PanelManager:OpenPanel:D2RHubRoomToolbar")
    });
    children.insert(
        0,
        serde_json::json!({
            "type": "TimerWidget",
            "name": "D2RHubRoomToolbarLauncher",
            "fields": {
                "time": 0.01,
                "message": "PanelManager:OpenPanel:D2RHubRoomToolbar"
            }
        }),
    );
    write_json_layout(mpq_directory, HUD_WARNINGS_LAYOUT, &hud)?;

    for (file_name, document) in [
        ("D2RHubRoomToolbarhd.json", room_toolbar_layout()),
        (
            "D2RHubQuickRecreateConfirmhd.json",
            quick_recreate_confirmation_layout(),
        ),
        ("D2RHubQuickRecreatehd.json", quick_recreate_layout()),
        (
            "D2RHubOpenCreateGamehd.json",
            room_panel_opener_layout(true),
        ),
        ("D2RHubOpenJoinGamehd.json", room_panel_opener_layout(false)),
    ] {
        write_json_layout(
            mpq_directory,
            &format!("{UI_LAYOUTS_DIRECTORY}/{file_name}"),
            &document,
        )?;
    }
    for file_name in [
        "D2RHubRoomToolbar.json",
        "D2RHubQuickRecreateConfirm.json",
        "D2RHubQuickRecreate.json",
        "D2RHubOpenCreateGame.json",
        "D2RHubOpenJoinGame.json",
    ] {
        write_json_layout(
            mpq_directory,
            &format!("{UI_LAYOUTS_DIRECTORY}/{file_name}"),
            &serde_json::json!({
                "type": "Panel",
                "name": file_name.trim_end_matches(".json")
            }),
        )?;
    }

    patch_room_form_layout(
        mpq_directory,
        storage,
        "data/global/ui/layouts/creategamepanelhd.json",
        "GameNameInput",
    )?;
    patch_room_form_layout(
        mpq_directory,
        storage,
        "data/global/ui/layouts/joingamepanelhd.json",
        "NameInput",
    )?;

    compatibility.push(AudioModCompatibility {
        target: "局内房间工具".to_string(),
        action: "add_in_game_create_join_and_recreate".to_string(),
        detail: "保留源 HUD 与原生建房/加入逻辑，仅追加常驻的“下一局 / 创建房间 / 加入房间”工具栏；下一局需要二次确认；打开表单的辅助面板沿用 MDK 的原生尺寸上下文，输入框沿用局内聊天框的键盘独占规则，阻止技能快捷键抢走房名、密码和描述按键；表单支持 Esc、关闭按钮和再次点击工具栏关闭。".to_string(),
    });
    Ok(true)
}

fn validate_misc(
    table: &TsvTable,
    include_runes: bool,
    items: &[SupportedItemDefinition],
) -> Result<(), String> {
    let code_index = table.column("code")?;
    let mut codes = items
        .iter()
        .map(|item| item.code.to_string())
        .collect::<Vec<_>>();
    if include_runes {
        codes.extend((1..=RUNE_COUNT).map(|rune_number| format!("r{rune_number:02}")));
    }
    for code in codes {
        table
            .rows
            .iter()
            .find(|row| row[code_index].eq_ignore_ascii_case(&code))
            .ok_or_else(|| format!("misc.txt 缺少追踪物品代码 {code}"))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SoundRole {
    RuneGroundHeartbeat,
    AreaAmbience,
    TerrorZoneProbe,
}

fn configure_sound_row(table: &TsvTable, row: &mut [String], role: SoundRole) {
    let is_ambience = matches!(role, SoundRole::AreaAmbience);
    let is_terror_probe = matches!(role, SoundRole::TerrorZoneProbe);
    for (column, value) in [
        ("Redirect", ""),
        ("Volume Min", "255"),
        ("Volume Max", "255"),
        ("Pitch Min", "100"),
        ("Pitch Max", "100"),
        ("Group Size", "0"),
        ("Group Weight", "0"),
        ("Loop", if is_ambience { "1" } else { "0" }),
        ("Duration", "0"),
        ("Delay", "0"),
        ("Defer Inst", "0"),
        ("Stop Inst", "0"),
        ("Compound", "0"),
        ("Stream", "0"),
        ("Tracking", "0"),
        ("Is2D", if is_terror_probe { "0" } else { "1" }),
        ("IsAmbientScene", if is_ambience { "1" } else { "0" }),
        ("IsAmbientEvent", if is_terror_probe { "1" } else { "0" }),
    ] {
        set_if_present(table, row, column, value);
    }
    if is_terror_probe {
        for (column, value) in [("Falloff", "0"), ("Priority", "255"), ("Solo", "0")] {
            set_if_present(table, row, column, value);
        }
    }
}

fn rune_unit_definition_candidates(mpq_directory: &Path, rune_number: u32) -> Vec<PathBuf> {
    let rune_name = rune_data::RUNE_NAMES_EN[(rune_number - 1) as usize].to_ascii_lowercase();
    ["rune", "runes"]
        .into_iter()
        .map(|folder| {
            mpq_directory.join(format!("data/hd/items/misc/{folder}/{rune_name}_rune.json"))
        })
        .collect()
}

fn parse_json_value(text: &str) -> Result<serde_json::Value, String> {
    let normalized = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str(normalized) {
        Ok(document) => Ok(document),
        Err(strict_error) => json5::from_str(normalized).map_err(|relaxed_error| {
            format!("严格 JSON: {strict_error}；宽松 JSON5: {relaxed_error}")
        }),
    }
}

fn read_json_asset(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    internal_path: &str,
) -> Result<(serde_json::Value, String), String> {
    let normalized = internal_path.replace('\\', "/");
    let local = mpq_directory.join(&normalized);
    let (text, source) = if local.is_file() {
        (read_utf8(&local)?, format!("Mod:{}", normalized))
    } else if let Some(storage) = storage {
        (
            read_casc_utf8(storage, &normalized)?,
            format!("CASC:{normalized}"),
        )
    } else {
        return Err(format!(
            "源 Mod 未包含 {normalized}，且没有可用的 D2R CASC；无法保留原状态机"
        ));
    };
    let document =
        parse_json_value(&text).map_err(|error| format!("解析 JSON 资源失败 {source}: {error}"))?;
    Ok((document, source))
}

fn read_casc_json_asset(
    storage: &casc_core::Storage,
    internal_path: &str,
) -> Result<serde_json::Value, String> {
    let normalized = internal_path.replace('\\', "/");
    let text = read_casc_utf8(storage, &normalized)?;
    parse_json_value(&text)
        .map_err(|error| format!("解析 D2R CASC JSON 资源失败 {normalized}: {error}"))
}

fn state_transition_exists(document: &serde_json::Value, from: i64, to: i64) -> bool {
    document
        .get("transitions")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|groups| {
            groups.iter().any(|group| {
                group.get("from").and_then(serde_json::Value::as_i64) == Some(from)
                    && group
                        .get("settings")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|settings| {
                            settings.iter().any(|setting| {
                                setting.get("to").and_then(serde_json::Value::as_i64) == Some(to)
                            })
                        })
            })
        })
}

fn patch_rune_unit_definitions(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<Vec<RuneStatePlan>, String> {
    let mut plans = Vec::with_capacity(RUNE_COUNT as usize);
    for rune_number in 1..=RUNE_COUNT {
        let rune_name = rune_data::RUNE_NAMES_EN[(rune_number - 1) as usize].to_ascii_lowercase();
        let relative = format!("data/hd/items/misc/rune/{rune_name}_rune.json");
        let path = rune_unit_definition_candidates(mpq_directory, rune_number)
            .into_iter()
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| mpq_directory.join(&relative));
        let mut document: serde_json::Value = if path.is_file() {
            parse_json_value(&read_utf8(&path)?)
                .map_err(|error| format!("解析 HD 符文实体失败 {}: {error}", path.display()))?
        } else {
            let (document, _) = read_json_asset(mpq_directory, storage, &relative)?;
            document
        };
        let entities = document
            .get_mut("entities")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| format!("HD 符文实体缺少 entities 数组: {}", path.display()))?;
        let components = entities
            .iter_mut()
            .filter_map(|entity| entity.get_mut("components"))
            .filter_map(serde_json::Value::as_array_mut)
            .find(|components| {
                components.iter().any(|component| {
                    component.get("type").and_then(serde_json::Value::as_str)
                        == Some("UnitRootComponent")
                })
            })
            .ok_or_else(|| format!("HD 符文实体缺少 UnitRootComponent: {}", path.display()))?;
        components.retain(|component| {
            !(component.get("type").and_then(serde_json::Value::as_str)
                == Some("AudioEmitterComponent")
                && component
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| name.starts_with("D2RHub_Rune_")))
        });
        let unit_root = components
            .iter_mut()
            .find(|component| {
                component.get("type").and_then(serde_json::Value::as_str)
                    == Some("UnitRootComponent")
            })
            .ok_or_else(|| format!("HD 符文缺少 UnitRootComponent: {}", path.display()))?;
        let original_state_machine = unit_root
            .get("state_machine_filename")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("HD 符文缺少落地状态机路径: {}", path.display()))?
            .to_string();
        let normalized_original = original_state_machine.replace('\\', "/");
        if normalized_original.contains("/d2rhub_audio/")
            || normalized_original.contains("/audio_telemetry/")
        {
            return Err(format!(
                "#{rune_number:02} 已指向旧版 D2RHub 状态机；请选原始 Mod，而不是加工后的输出"
            ));
        }
        let (mut state_machine, state_machine_source) =
            read_json_asset(mpq_directory, storage, &normalized_original)?;
        let (flippy_index, flippy_id, ground_id, original_audio_id) = {
            let states = state_machine
                .get("states")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("状态机缺少 states 数组: {state_machine_source}"))?;
            let flippy_index = states
                .iter()
                .position(|state| {
                    state
                        .get("_name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case("Flippy"))
                })
                .ok_or_else(|| format!("状态机没有 Flippy 状态: {state_machine_source}"))?;
            let ground_index = states
                .iter()
                .position(|state| {
                    state
                        .get("_name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case("Ground"))
                })
                .ok_or_else(|| format!("状态机没有 Ground 状态: {state_machine_source}"))?;
            let flippy_id = states[flippy_index]
                .get("stateId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| format!("Flippy 状态缺少 stateId: {state_machine_source}"))?;
            let ground_id = states[ground_index]
                .get("stateId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| format!("Ground 状态缺少 stateId: {state_machine_source}"))?;
            let original_audio_id = states[flippy_index]
                .get("audioId")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            (flippy_index, flippy_id, ground_id, original_audio_id)
        };
        if !state_transition_exists(&state_machine, flippy_id, ground_id)
            || !state_transition_exists(&state_machine, ground_id, flippy_id)
        {
            return Err(format!(
                "#{rune_number:02} 的原状态机没有完整 Flippy ↔ Ground 循环，兼容优先模式拒绝改写转场: {state_machine_source}"
            ));
        }
        state_machine["states"][flippy_index]["audioId"] =
            serde_json::Value::String(format!("audio_telemetry_r{rune_number:02}"));
        if let Some(name) = state_machine.get_mut("name") {
            *name =
                serde_json::Value::String(format!("audio_telemetry_r{rune_number:02}_compatible"));
        }
        let telemetry_state_machine =
            format!("data/hd/items/audio_telemetry/runes/r{rune_number:02}_ground_heartbeat.json");
        unit_root["state_machine_filename"] =
            serde_json::Value::String(telemetry_state_machine.clone());

        let dependencies = document
            .get_mut("dependencies")
            .and_then(|value| value.get_mut("json"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| format!("HD 符文缺少 dependencies.json: {}", path.display()))?;
        if let Some(reference) = dependencies.iter_mut().find(|reference| {
            reference.get("path").and_then(serde_json::Value::as_str)
                == Some(original_state_machine.as_str())
        }) {
            reference["path"] = serde_json::Value::String(telemetry_state_machine.clone());
        } else {
            dependencies.push(serde_json::json!({ "path": telemetry_state_machine }));
        }

        write_file(
            &mpq_directory.join(telemetry_state_machine.replace('/', "\\")),
            serde_json::to_vec_pretty(&state_machine)
                .map_err(|error| format!("序列化符文地面心跳状态机失败: {error}"))?,
        )?;
        write_file(
            &path,
            serde_json::to_vec_pretty(&document)
                .map_err(|error| format!("序列化 HD 符文实体失败: {error}"))?,
        )?;
        compatibility.push(AudioModCompatibility {
            target: format!("#{rune_number:02} 符文状态机"),
            action: if original_audio_id.is_some() {
                "mix_original_audio".to_string()
            } else {
                "attach_in_place".to_string()
            },
            detail: format!(
                "从 {state_machine_source} 克隆；保留原动画、VFX、依赖和双向转场，仅将 Flippy.audioId 指向独立声纹{}。",
                original_audio_id
                    .as_deref()
                    .map(|audio| format!("，原声音 {audio} 将混入新资源"))
                    .unwrap_or_default()
            ),
        });
        plans.push(RuneStatePlan {
            rune_number,
            original_audio_id,
        });
    }
    Ok(plans)
}

fn item_asset(document: &serde_json::Value, code: &str) -> Option<String> {
    document.as_array()?.iter().find_map(|row| {
        let object = row.as_object()?;
        object.iter().find_map(|(candidate, value)| {
            candidate.eq_ignore_ascii_case(code).then(|| {
                value
                    .get("asset")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })?
        })
    })
}

fn set_item_asset(document: &mut serde_json::Value, code: &str, asset: &str) -> Result<(), String> {
    let rows = document
        .as_array_mut()
        .ok_or_else(|| "data/hd/items/items.json 不是数组".to_string())?;
    for row in rows {
        let Some(object) = row.as_object_mut() else {
            continue;
        };
        let Some(key) = object
            .keys()
            .find(|candidate| candidate.eq_ignore_ascii_case(code))
            .cloned()
        else {
            continue;
        };
        let value = object
            .get_mut(&key)
            .ok_or_else(|| format!("items.json 中 {code} 映射异常"))?;
        let target = value
            .as_object_mut()
            .ok_or_else(|| format!("items.json 中 {code} 不是对象"))?;
        target.insert(
            "asset".to_string(),
            serde_json::Value::String(asset.to_string()),
        );
        return Ok(());
    }
    Err(format!("items.json 缺少物品代码 {code}"))
}

fn copy_item_ui_sprites(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    source_assets: &[&str],
    cloned_asset: &str,
) -> Result<usize, String> {
    let target_stem = cloned_asset.replace('\\', "/");
    let mut copied = 0;
    for suffix in std::iter::once(String::new()).chain((1..=16).map(|number| number.to_string())) {
        for ending in [".sprite", ".lowend.sprite"] {
            let mut bytes = None;
            for asset in source_assets {
                let source_stem = asset.replace('\\', "/");
                let source = format!("data/hd/global/ui/items/misc/{source_stem}{suffix}{ending}");
                let local = mpq_directory.join(source.replace('/', "\\"));
                bytes = if local.is_file() {
                    Some(std::fs::read(&local).map_err(|error| {
                        format!("读取源 Mod 物品图标失败 {}: {error}", local.display())
                    })?)
                } else if let Some(storage) = storage {
                    storage.read(&casc_path(&source)).ok()
                } else {
                    None
                };
                if bytes.is_some() {
                    break;
                }
            }
            let Some(bytes) = bytes else {
                continue;
            };
            let target = format!("data/hd/global/ui/items/misc/{target_stem}{suffix}{ending}");
            write_file(&mpq_directory.join(target.replace('/', "\\")), bytes)?;
            copied += 1;
        }
    }
    if copied == 0 {
        return Err(format!(
            "无法从候选映射 [{}] 找到 HD 背包/仓库图标资源；拒绝生成不可见物品",
            source_assets.join(", ")
        ));
    }
    Ok(copied)
}

fn read_item_entity_asset(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    asset: &str,
) -> Result<(serde_json::Value, String, String), String> {
    let normalized = asset.replace('\\', "/").trim_matches('/').to_string();
    let mut candidates = Vec::new();
    if normalized.starts_with("misc/") {
        candidates.push(format!("data/hd/items/{normalized}.json"));
    } else {
        // misc item mappings are relative to data/hd/items/misc.
        candidates.push(format!("data/hd/items/misc/{normalized}.json"));
        candidates.push(format!("data/hd/items/{normalized}.json"));
    }
    let mut errors = Vec::new();
    for candidate in candidates {
        match read_json_asset(mpq_directory, storage, &candidate) {
            Ok((document, source)) => return Ok((document, source, candidate)),
            Err(error) => errors.push(error),
        }
    }
    Err(format!(
        "无法解析 items.json 资源映射 {asset}: {}",
        errors.join("；")
    ))
}

fn resolve_item_entity_asset(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    preferred_asset: &str,
    baseline_asset: Option<&str>,
) -> Result<ResolvedItemEntityAsset, String> {
    match read_item_entity_asset(mpq_directory, storage, preferred_asset) {
        Ok((document, source, _)) => Ok(ResolvedItemEntityAsset {
            document,
            source,
            asset: preferred_asset.to_string(),
            used_baseline_mapping: false,
            preferred_error: None,
        }),
        Err(preferred_error) => {
            let baseline_asset = baseline_asset
                .map(str::trim)
                .filter(|asset| !asset.is_empty())
                .filter(|asset| !asset.eq_ignore_ascii_case(preferred_asset));
            let Some(baseline_asset) = baseline_asset else {
                return Err(preferred_error);
            };
            match read_item_entity_asset(mpq_directory, storage, baseline_asset) {
                Ok((document, source, _)) => Ok(ResolvedItemEntityAsset {
                    document,
                    source,
                    asset: baseline_asset.to_string(),
                    used_baseline_mapping: true,
                    preferred_error: Some(preferred_error),
                }),
                Err(baseline_error) => Err(format!(
                    "源 Mod 映射与游戏基线映射均无法解析。源映射：{preferred_error}；基线映射：{baseline_error}"
                )),
            }
        }
    }
}

fn strip_item_formatting(value: &str) -> String {
    let base = value.split('|').next().unwrap_or(value);
    let mut output = String::new();
    let mut characters = base.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{00ff}' && characters.peek() == Some(&'c') {
            let _ = characters.next();
            // D2 color controls are normally one character (ÿc1). A few Mods
            // use a semicolon tag (ÿc;SC); discard that compact tag as well.
            if characters.peek() == Some(&';') {
                let _ = characters.next();
                while characters
                    .peek()
                    .is_some_and(|next| next.is_ascii_uppercase())
                {
                    let _ = characters.next();
                }
            } else {
                let _ = characters.next();
            }
            continue;
        }
        output.push(character);
    }
    output.trim().trim_matches('★').trim().to_string()
}

fn load_item_localization(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
) -> Result<HashMap<String, (String, String)>, String> {
    let relative = "data/local/lng/strings/item-names.json";
    // Prefer the game's own localization so a visual Mod's decorations do not
    // leak into persisted statistics. Fall back to the copied Mod if necessary.
    let text = if let Some(storage) = storage {
        read_casc_utf8(storage, relative).or_else(|_| {
            let local = mpq_directory.join(relative);
            read_utf8(&local)
        })?
    } else {
        read_utf8(&mpq_directory.join(relative))?
    };
    let rows: serde_json::Value =
        serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
            .map_err(|error| format!("解析物品名称本地化失败: {error}"))?;
    Ok(rows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let key = row
                .get("Key")
                .or_else(|| row.get("key"))
                .and_then(serde_json::Value::as_str)?;
            let english = row
                .get("enUS")
                .and_then(serde_json::Value::as_str)
                .map(strip_item_formatting)
                .filter(|value| !value.is_empty())?;
            let chinese = row
                .get("zhCN")
                .or_else(|| row.get("zhTW"))
                .and_then(serde_json::Value::as_str)
                .map(strip_item_formatting)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| english.clone());
            Some((key.to_ascii_lowercase(), (chinese, english)))
        })
        .collect())
}

fn patch_item_unit_definitions(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    items_document: &mut serde_json::Value,
    baseline_items_document: Option<&serde_json::Value>,
    definitions: &[SupportedItemDefinition],
    localization: &HashMap<String, (String, String)>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<Vec<ItemStatePlan>, String> {
    let mut plans = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let original_asset = item_asset(items_document, definition.code)
            .ok_or_else(|| format!("items.json 缺少物品代码 {}", definition.code))?;
        let normalized_asset = original_asset.replace('\\', "/");
        if normalized_asset.contains("d2rhub_audio/")
            || normalized_asset.contains("audio_telemetry/")
        {
            return Err(format!(
                "物品 {} 已指向旧版 D2RHub 资源；请选原始 Mod，而不是加工后的输出",
                definition.code
            ));
        }
        let baseline_asset =
            baseline_items_document.and_then(|document| item_asset(document, definition.code));
        let resolved_entity = resolve_item_entity_asset(
            mpq_directory,
            storage,
            &original_asset,
            baseline_asset.as_deref(),
        )?;
        let entity_source = resolved_entity.source.clone();
        let entity_asset = resolved_entity.asset.clone();
        let used_baseline_mapping = resolved_entity.used_baseline_mapping;
        let preferred_error = resolved_entity.preferred_error.clone();
        let mut document = resolved_entity.document;
        let entities = document
            .get_mut("entities")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| format!("物品实体缺少 entities 数组: {entity_source}"))?;
        let components = entities
            .iter_mut()
            .filter_map(|entity| entity.get_mut("components"))
            .filter_map(serde_json::Value::as_array_mut)
            .find(|components| {
                components.iter().any(|component| {
                    component.get("type").and_then(serde_json::Value::as_str)
                        == Some("UnitRootComponent")
                })
            })
            .ok_or_else(|| format!("物品实体缺少 UnitRootComponent: {entity_source}"))?;
        let unit_root = components
            .iter_mut()
            .find(|component| {
                component.get("type").and_then(serde_json::Value::as_str)
                    == Some("UnitRootComponent")
            })
            .ok_or_else(|| format!("物品实体缺少 UnitRootComponent: {entity_source}"))?;
        let original_state_machine = unit_root
            .get("state_machine_filename")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("物品实体缺少落地状态机路径: {entity_source}"))?
            .to_string();
        let normalized_original = original_state_machine.replace('\\', "/");
        if normalized_original.contains("/d2rhub_audio/")
            || normalized_original.contains("/audio_telemetry/")
        {
            return Err(format!(
                "物品 {} 已指向旧版 D2RHub 状态机；请选原始 Mod",
                definition.code
            ));
        }
        let (mut state_machine, state_machine_source) =
            read_json_asset(mpq_directory, storage, &normalized_original)?;
        let (flippy_index, flippy_id, ground_id, original_audio_id) = {
            let states = state_machine
                .get("states")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("状态机缺少 states 数组: {state_machine_source}"))?;
            let flippy_index = states
                .iter()
                .position(|state| {
                    state
                        .get("_name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case("Flippy"))
                })
                .ok_or_else(|| format!("状态机没有 Flippy 状态: {state_machine_source}"))?;
            let ground_index = states
                .iter()
                .position(|state| {
                    state
                        .get("_name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case("Ground"))
                })
                .ok_or_else(|| format!("状态机没有 Ground 状态: {state_machine_source}"))?;
            let flippy_id = states[flippy_index]
                .get("stateId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| format!("Flippy 状态缺少 stateId: {state_machine_source}"))?;
            let ground_id = states[ground_index]
                .get("stateId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| format!("Ground 状态缺少 stateId: {state_machine_source}"))?;
            let original_audio_id = states[flippy_index]
                .get("audioId")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            (flippy_index, flippy_id, ground_id, original_audio_id)
        };
        if !state_transition_exists(&state_machine, flippy_id, ground_id)
            || !state_transition_exists(&state_machine, ground_id, flippy_id)
        {
            return Err(format!(
                "物品 {} 的原状态机没有完整 Flippy ↔ Ground 循环，兼容优先模式拒绝改写: {state_machine_source}",
                definition.code
            ));
        }

        let sound_id = format!("audio_telemetry_i{:02}", definition.item_id);
        state_machine["states"][flippy_index]["audioId"] = serde_json::Value::String(sound_id);
        if let Some(name) = state_machine.get_mut("name") {
            *name = serde_json::Value::String(format!(
                "audio_telemetry_i{:02}_{}_compatible",
                definition.item_id, definition.code
            ));
        }
        let telemetry_state_machine = format!(
            "data/hd/items/audio_telemetry/state_machines/i{:02}_{}_ground_heartbeat.json",
            definition.item_id, definition.code
        );
        unit_root["state_machine_filename"] =
            serde_json::Value::String(telemetry_state_machine.clone());
        let dependencies = document
            .get_mut("dependencies")
            .and_then(|value| value.get_mut("json"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| format!("物品实体缺少 dependencies.json: {entity_source}"))?;
        if let Some(reference) = dependencies.iter_mut().find(|reference| {
            reference.get("path").and_then(serde_json::Value::as_str)
                == Some(original_state_machine.as_str())
        }) {
            reference["path"] = serde_json::Value::String(telemetry_state_machine.clone());
        } else {
            dependencies.push(serde_json::json!({ "path": telemetry_state_machine }));
        }

        let cloned_asset = format!(
            "audio_telemetry/items/i{:02}_{}",
            definition.item_id, definition.code
        );
        let cloned_entity_path =
            mpq_directory.join(format!("data/hd/items/misc/{cloned_asset}.json"));
        write_file(
            &mpq_directory.join(telemetry_state_machine.replace('/', "\\")),
            serde_json::to_vec_pretty(&state_machine)
                .map_err(|error| format!("序列化物品地面状态机失败: {error}"))?,
        )?;
        write_file(
            &cloned_entity_path,
            serde_json::to_vec_pretty(&document)
                .map_err(|error| format!("序列化物品实体失败: {error}"))?,
        )?;
        let mut sprite_assets = vec![original_asset.as_str()];
        if !entity_asset.eq_ignore_ascii_case(&original_asset) {
            sprite_assets.push(entity_asset.as_str());
        }
        let copied_sprites =
            copy_item_ui_sprites(mpq_directory, storage, &sprite_assets, &cloned_asset)?;
        set_item_asset(items_document, definition.code, &cloned_asset)?;

        let (name, name_en) = localization
            .get(&definition.code.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| {
                (
                    definition.fallback_name.to_string(),
                    definition.fallback_name_en.to_string(),
                )
            });
        let entry = ItemCatalogEntry {
            item_id: definition.item_id,
            code: definition.code.to_string(),
            category: definition.category.to_string(),
            name,
            name_en,
            asset: cloned_asset,
        };
        compatibility.push(AudioModCompatibility {
            target: format!("{} ({})", entry.name, entry.code),
            action: if used_baseline_mapping {
                "clone_with_baseline_entity_mapping".to_string()
            } else if original_audio_id.is_some() {
                "mix_original_audio".to_string()
            } else {
                "clone_and_attach".to_string()
            },
            detail: format!(
                "从 {entity_source} 克隆为独立实体；保留原模型、VFX、动画、依赖和双向转场，按 [{}] 的优先级复制 {copied_sprites} 个背包/仓库 sprite 资源，仅替换克隆体的 Flippy.audioId{}{}。",
                sprite_assets.join(", "),
                original_audio_id
                    .as_deref()
                    .map(|audio| format!("，并混入原声音 {audio}"))
                    .unwrap_or_default(),
                preferred_error
                    .as_deref()
                    .map(|error| format!("；源映射缺少可解析世界实体，已按同一物品代码使用游戏基线映射 {entity_asset}。原解析信息：{error}"))
                    .unwrap_or_default()
            ),
        });
        plans.push(ItemStatePlan {
            entry,
            original_audio_id,
        });
    }
    Ok(plans)
}

fn patch_sounds(
    mut table: TsvTable,
    rune_plans: &[RuneStatePlan],
    item_plans: &[ItemStatePlan],
    areas: &[AreaCatalogEntry],
    area_ambience_filenames: &HashMap<u32, String>,
    compatibility: &mut Vec<AudioModCompatibility>,
) -> Result<(TsvTable, Vec<SoundDefinition>), String> {
    let mut next_index = table.max_number("*Index")? + 1;
    let rune_template = table
        .row_by("Sound", "item_rune_hd")
        .or_else(|_| table.row_by("Sound", "item_rune"))?;
    let area_template = table
        .row_by("Sound", "wilderness_day_2_hd")
        .or_else(|_| table.row_by("Sound", "scene_wilderness_day"))
        .or_else(|_| {
            let ambient_index = table.column("IsAmbientScene")?;
            table
                .rows
                .iter()
                .find(|row| row[ambient_index] == "1")
                .cloned()
                .ok_or_else(|| "sounds.txt 中找不到持续环境音模板".to_string())
        })?;
    let mut definitions =
        Vec::with_capacity(rune_plans.len() + item_plans.len() + areas.len() + 48);

    for plan in rune_plans {
        let rune_number = plan.rune_number;
        let marker = TelemetryMarker::Rune { rune_number };
        let sound = format!("audio_telemetry_r{rune_number:02}");
        let relative_path = format!("audio_telemetry\\runes\\r{rune_number:02}.flac");
        let (mut row, source_filename) = if let Some(original_audio_id) = &plan.original_audio_id {
            let mut current = original_audio_id.clone();
            let sound_column = table.column("Sound")?;
            let filename_column = table.column("FileName")?;
            let redirect_column = table.column("Redirect").ok();
            let compound_column = table.column("Compound").ok();
            let mut resolved = None;
            for _ in 0..8 {
                let candidate = table
                    .rows
                    .iter()
                    .find(|row| row[sound_column].eq_ignore_ascii_case(&current))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "#{rune_number:02} 的 Flippy.audioId={original_audio_id} 在 sounds.txt 中不存在"
                        )
                    })?;
                if let Some(redirect) = redirect_column
                    .and_then(|index| candidate.get(index))
                    .map(String::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    current = redirect.to_string();
                    continue;
                }
                if compound_column
                    .and_then(|index| candidate.get(index))
                    .is_some_and(|value| !value.trim().is_empty() && value.trim() != "0")
                {
                    return Err(format!(
                        "#{rune_number:02} 的原声音 {original_audio_id} 是 Compound 声音；兼容优先模式无法证明单文件混音无损"
                    ));
                }
                let filename = candidate[filename_column].trim().to_string();
                if filename.is_empty() {
                    return Err(format!(
                        "#{rune_number:02} 的原声音 {original_audio_id} 没有可混音的 FileName"
                    ));
                }
                resolved = Some((candidate, filename));
                break;
            }
            resolved.ok_or_else(|| {
                format!("#{rune_number:02} 的原声音 {original_audio_id} Redirect 链过长")
            })?
        } else {
            (rune_template.clone(), String::new())
        };
        table.set(&mut row, "Sound", &sound)?;
        table.set(&mut row, "*Index", next_index.to_string())?;
        table.set(&mut row, "FileName", &relative_path)?;
        set_if_present(&table, &mut row, "Redirect", "");
        if plan.original_audio_id.is_none() {
            configure_sound_row(&table, &mut row, SoundRole::RuneGroundHeartbeat);
        }
        table.rows.push(row);
        definitions.push(SoundDefinition {
            marker,
            group: SoundAssetGroup::Rune,
            sound,
            relative_path,
            source_filename: (!source_filename.is_empty()).then_some(source_filename),
            output_root: "data/hd/global/sfx",
        });
        next_index += 1;
    }

    for plan in item_plans {
        let marker = TelemetryMarker::Item {
            item_id: plan.entry.item_id,
        };
        let sound = format!("audio_telemetry_i{:02}", plan.entry.item_id);
        let relative_path = format!(
            "audio_telemetry\\items\\i{:02}_{}.flac",
            plan.entry.item_id, plan.entry.code
        );
        let (mut row, source_filename) = if let Some(original_audio_id) = &plan.original_audio_id {
            let mut current = original_audio_id.clone();
            let sound_column = table.column("Sound")?;
            let filename_column = table.column("FileName")?;
            let redirect_column = table.column("Redirect").ok();
            let compound_column = table.column("Compound").ok();
            let mut resolved = None;
            for _ in 0..8 {
                let candidate = table
                    .rows
                    .iter()
                    .find(|row| row[sound_column].eq_ignore_ascii_case(&current))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "物品 {} 的 Flippy.audioId={original_audio_id} 在 sounds.txt 中不存在",
                            plan.entry.code
                        )
                    })?;
                if let Some(redirect) = redirect_column
                    .and_then(|index| candidate.get(index))
                    .map(String::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    current = redirect.to_string();
                    continue;
                }
                if compound_column
                    .and_then(|index| candidate.get(index))
                    .is_some_and(|value| !value.trim().is_empty() && value.trim() != "0")
                {
                    return Err(format!(
                        "物品 {} 的原声音 {original_audio_id} 是 Compound 声音；兼容优先模式无法证明单文件混音无损",
                        plan.entry.code
                    ));
                }
                let filename = candidate[filename_column].trim().to_string();
                if filename.is_empty() {
                    return Err(format!(
                        "物品 {} 的原声音 {original_audio_id} 没有可混音的 FileName",
                        plan.entry.code
                    ));
                }
                resolved = Some((candidate, filename));
                break;
            }
            resolved.ok_or_else(|| {
                format!(
                    "物品 {} 的原声音 {original_audio_id} Redirect 链过长",
                    plan.entry.code
                )
            })?
        } else {
            (rune_template.clone(), String::new())
        };
        table.set(&mut row, "Sound", &sound)?;
        table.set(&mut row, "*Index", next_index.to_string())?;
        table.set(&mut row, "FileName", &relative_path)?;
        set_if_present(&table, &mut row, "Redirect", "");
        if plan.original_audio_id.is_none() {
            configure_sound_row(&table, &mut row, SoundRole::RuneGroundHeartbeat);
        }
        table.rows.push(row);
        definitions.push(SoundDefinition {
            marker,
            group: SoundAssetGroup::Item,
            sound,
            relative_path,
            source_filename: (!source_filename.is_empty()).then_some(source_filename),
            output_root: "data/hd/global/sfx",
        });
        next_index += 1;
    }

    for area in areas {
        let marker = TelemetryMarker::Area {
            area_id: area.area_id,
        };
        let sound = format!("audio_telemetry_a{}", area.area_id);
        let relative_path = format!("audio_telemetry\\areas\\a{}.flac", area.area_id);
        let mut row = area_template.clone();
        table.set(&mut row, "Sound", &sound)?;
        table.set(&mut row, "*Index", next_index.to_string())?;
        table.set(&mut row, "FileName", &relative_path)?;
        configure_sound_row(&table, &mut row, SoundRole::AreaAmbience);
        table.rows.push(row);
        definitions.push(SoundDefinition {
            marker,
            group: SoundAssetGroup::Area,
            sound,
            relative_path,
            source_filename: area_ambience_filenames.get(&area.area_id).cloned(),
            output_root: "data/hd/global/sfx",
        });
        next_index += 1;
    }

    let terror_probe_template = table
        .row_by("Sound", "desecrated_enter_hd")
        .or_else(|_| table.row_by("Sound", "desecrated_enter"))
        .unwrap_or_else(|_| area_template.clone());
    for (sound, channel) in [
        (TERROR_PROBE_SD_SOUND, "sfx/ambient/event-3d_sd"),
        (TERROR_PROBE_HD_SOUND, "sfx/ambient/event-3d_hd"),
    ] {
        let mut row = terror_probe_template.clone();
        table.set(&mut row, "Sound", sound)?;
        table.set(&mut row, "*Index", next_index.to_string())?;
        table.set(&mut row, "FileName", TERROR_PROBE_RELATIVE_PATH)?;
        set_if_present(&table, &mut row, "Channel", channel);
        configure_sound_row(&table, &mut row, SoundRole::TerrorZoneProbe);
        table.rows.push(row);
        next_index += 1;
    }

    // The shared ambient event is only a delayed heartbeat. Primary TZ
    // detection must ride the one-shot sound that D2R plays immediately when
    // entering a desecrated zone. Follow redirects so SD->HD layouts patch the
    // actual terminal sound once while preserving the original event names,
    // channels and audible content.
    let sound_column = table.column("Sound")?;
    let filename_column = table.column("FileName")?;
    let redirect_column = table.column("Redirect").ok();
    let mut terror_entry_rows = Vec::new();
    for entry_sound in ["desecrated_enter_hd", "desecrated_enter"] {
        let mut current = entry_sound.to_string();
        let mut visited = Vec::new();
        let mut found_entry = false;
        let mut resolved = false;
        for _ in 0..8 {
            let normalized = current.to_ascii_lowercase();
            if visited.contains(&normalized) {
                return Err(format!(
                    "恐怖区域入场声音 {entry_sound} 的 Redirect 形成循环: {} -> {current}",
                    visited.join(" -> ")
                ));
            }
            visited.push(normalized);
            let row_index = table
                .rows
                .iter()
                .position(|row| row[sound_column].eq_ignore_ascii_case(&current))
                .ok_or_else(|| {
                    if found_entry {
                        format!("恐怖区域入场声音 {entry_sound} 重定向到不存在的声音 {current}")
                    } else {
                        String::new()
                    }
                });
            let row_index = match row_index {
                Ok(row_index) => row_index,
                Err(_) if !found_entry => break,
                Err(error) => return Err(error),
            };
            found_entry = true;
            let redirect = redirect_column
                .and_then(|column| table.rows[row_index].get(column))
                .map(|value| value.trim())
                .filter(|value| !value.is_empty());
            if let Some(redirect) = redirect {
                current = redirect.to_string();
                continue;
            }
            if !terror_entry_rows.contains(&row_index) {
                terror_entry_rows.push(row_index);
            }
            resolved = true;
            break;
        }
        if found_entry && !resolved {
            return Err(format!(
                "恐怖区域入场声音 {entry_sound} 的 Redirect 链超过 8 层"
            ));
        }
    }
    if terror_entry_rows.is_empty() {
        return Err(
            "sounds.txt 缺少恐怖区域即时入场声音 desecrated_enter_hd / desecrated_enter"
                .to_string(),
        );
    }
    for row_index in terror_entry_rows {
        let sound = table.rows[row_index][sound_column].trim().to_string();
        let source_filename = table.rows[row_index][filename_column].trim().to_string();
        if source_filename.is_empty() {
            return Err(format!("恐怖区域入场声音 {sound} 没有可保留的 FileName"));
        }
        let relative_path = format!("audio_telemetry\\terror\\{sound}.flac");
        table.rows[row_index][filename_column] = relative_path.clone();
        definitions.push(SoundDefinition {
            marker: TelemetryMarker::Area {
                area_id: TERROR_PROBE_MARKER_AREA_ID,
            },
            group: SoundAssetGroup::Terror,
            sound: sound.clone(),
            relative_path,
            source_filename: Some(source_filename.clone()),
            output_root: "data/hd/global/sfx",
        });
        compatibility.push(AudioModCompatibility {
            target: format!("恐怖区域即时入场声音 {sound}"),
            action: "mix_immediate_terror_presence_marker".to_string(),
            detail: format!(
                "保留原入场声音 {source_filename}、事件名、通道与播放参数，只向同一次立即播放中混入固定 {TERROR_MARKER_GAIN_DB:.0} dBFS 的 TZ 1023 声纹；延迟环境事件仅作为后续兜底。"
            ),
        });
    }

    let hd_opt_out_column = table.column("HDOptOut").ok();
    let loop_column = table.column("Loop").ok();
    let frontend_rows = table
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let sound = row[sound_column].trim();
            let normalized = sound.to_ascii_lowercase();
            let stable_frontend_event = normalized.starts_with("event_fe_act_")
                && (normalized.starts_with("event_fe_act_4_")
                    || loop_column
                        .and_then(|column| row.get(column))
                        .is_some_and(|value| value.trim() == "1"));
            let stable_frontend_scene =
                normalized.ends_with("_front_end") || normalized == "char_select_fe_fire_loop_hd";
            (sound.eq_ignore_ascii_case("music_options")
                || stable_frontend_event
                || stable_frontend_scene)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if !frontend_rows
        .iter()
        .any(|&index| table.rows[index][sound_column].eq_ignore_ascii_case("music_options"))
    {
        return Err("sounds.txt 缺少主界面稳定入口 music_options".to_string());
    }
    for row_index in frontend_rows {
        let sound = table.rows[row_index][sound_column].trim().to_string();
        let original_filename = table.rows[row_index][filename_column].trim().to_string();
        if original_filename.is_empty() {
            continue;
        }
        let source_filename = if sound.eq_ignore_ascii_case("music_options") {
            original_filename
                .strip_suffix(".flac")
                .map(|stem| format!("{stem}_hd.flac"))
                .unwrap_or_else(|| original_filename.clone())
        } else {
            original_filename.clone()
        };
        let stem = sound
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let relative_path = format!("audio_telemetry\\frontend\\{stem}.flac");
        table.rows[row_index][filename_column] = relative_path.clone();
        if sound.eq_ignore_ascii_case("music_options") {
            if let Some(column) = hd_opt_out_column {
                table.rows[row_index][column] = "1".to_string();
            }
        }
        definitions.push(SoundDefinition {
            marker: TelemetryMarker::Frontend,
            group: SoundAssetGroup::Frontend,
            sound: sound.clone(),
            relative_path,
            source_filename: Some(source_filename.clone()),
            output_root: if sound.eq_ignore_ascii_case("music_options") {
                "data/global/music"
            } else {
                "data/hd/global/sfx"
            },
        });
        compatibility.push(AudioModCompatibility {
            target: format!("主界面声音 {sound}"),
            action: "mix_original_audio".to_string(),
            detail: format!(
                "保留声音条目的通道、音量、循环和优先级，仅把原资源 {source_filename} 克隆为独立主界面声纹；不修改恐惧区域声音条目。"
            ),
        });
    }
    Ok((table, definitions))
}

fn sanitize_scene_key(value: &str, area_id: u32) -> String {
    let mut output = String::new();
    let mut underscore = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
            underscore = false;
        } else if !underscore && !output.is_empty() {
            output.push('_');
            underscore = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        format!("area_{area_id}")
    } else {
        format!("area_{area_id}_{output}")
    }
}

#[cfg(test)]
fn collect_areas(levels: &TsvTable) -> Result<Vec<AreaCatalogEntry>, String> {
    collect_areas_localized(levels, &HashMap::new())
}

fn strip_level_annotation(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .rfind('[')
        .filter(|&index| trimmed.ends_with(']') && index > 0)
        .map(|index| trimmed[..index].trim().to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

fn load_area_localization(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
) -> Result<HashMap<String, (String, String)>, String> {
    let relative = "data/local/lng/strings/levels.json";
    let local = mpq_directory.join(relative);
    let text = if local.is_file() {
        read_utf8(&local)?
    } else if let Some(storage) = storage {
        read_casc_utf8(storage, relative)?
    } else {
        return Ok(HashMap::new());
    };
    let document = parse_json_value(&text)
        .map_err(|error| format!("解析地图本地化 levels.json 失败: {error}"))?;
    let rows = match document {
        serde_json::Value::Array(rows) => rows,
        _ => return Err("解析地图本地化 levels.json 失败: 顶层必须是数组".to_string()),
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let key = row.get("Key")?.as_str()?.trim();
            if key.is_empty() {
                return None;
            }
            let english = row
                .get("enUS")
                .and_then(serde_json::Value::as_str)
                .map(strip_level_annotation)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| key.to_string());
            let chinese = row
                .get("zhCN")
                .or_else(|| row.get("zhTW"))
                .and_then(serde_json::Value::as_str)
                .map(strip_level_annotation)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| english.clone());
            Some((key.to_ascii_lowercase(), (chinese, english)))
        })
        .collect())
}

fn collect_areas_localized(
    levels: &TsvTable,
    localization: &HashMap<String, (String, String)>,
) -> Result<Vec<AreaCatalogEntry>, String> {
    let id = levels.column("Id")?;
    let name = levels.column("Name")?;
    let level_name = levels.column("LevelName").unwrap_or(name);
    let mut areas = levels
        .rows
        .iter()
        .filter_map(|row| {
            let area_id = row[id].trim().parse::<u32>().ok()?;
            if !(1..=MAX_AREA_ID).contains(&area_id) {
                return None;
            }
            let internal_name = row[name].trim();
            let display_name = row[level_name].trim();
            if internal_name.is_empty() || internal_name.eq_ignore_ascii_case("null") {
                return None;
            }
            let lookup_key = if display_name.is_empty() {
                internal_name
            } else {
                display_name
            };
            let localized = localization.get(&lookup_key.to_ascii_lowercase());
            let scene_name = localized
                .map(|(chinese, _)| chinese.as_str())
                .unwrap_or(lookup_key);
            let scene_name_en = localized
                .map(|(_, english)| english.as_str())
                .unwrap_or(lookup_key);
            let kind = if internal_name.to_ascii_lowercase().contains(" - town")
                || matches!(area_id, 1 | 40 | 75 | 103 | 109)
            {
                LocationKind::Town
            } else {
                LocationKind::Wilderness
            };
            Some(AreaCatalogEntry {
                area_id,
                scene_key: sanitize_scene_key(scene_name, area_id),
                scene_name: scene_name.to_string(),
                scene_name_en: scene_name_en.to_string(),
                kind,
            })
        })
        .collect::<Vec<_>>();
    areas.sort_by_key(|area| area.area_id);
    areas.dedup_by_key(|area| area.area_id);
    if areas.is_empty() {
        return Err("levels.txt 中没有可编码的 Area Id".to_string());
    }
    Ok(areas)
}

fn patch_sound_environ_and_levels(
    mut environments: TsvTable,
    mut levels: TsvTable,
    areas: &[AreaCatalogEntry],
) -> Result<(TsvTable, TsvTable), String> {
    let level_id = levels.column("Id")?;
    let level_environment = levels.column("SoundEnv")?;
    let environment_index = environments.column("Index")?;
    let mut next_index = environments.max_number("Index")? + 1;
    let area_lookup = areas
        .iter()
        .map(|area| (area.area_id, area))
        .collect::<HashMap<_, _>>();

    for level_row in &mut levels.rows {
        let Some(area_id) = level_row[level_id].trim().parse::<u32>().ok() else {
            continue;
        };
        if !area_lookup.contains_key(&area_id) {
            continue;
        }
        let original_id = level_row[level_environment].trim();
        let mut row = environments
            .rows
            .iter()
            .find(|row| row[environment_index].trim() == original_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "soundenviron.txt 缺少 levels.txt Area {area_id} 使用的 Index={original_id}"
                )
            })?;
        environments.set(
            &mut row,
            "Handle",
            format!("ESOUNDENVIRON_AUDIO_TELEMETRY_A{area_id}"),
        )?;
        environments.set(&mut row, "Index", next_index.to_string())?;
        let sound = format!("audio_telemetry_a{area_id}");
        for column in [
            "Day Ambience",
            "HD Day Ambience",
            "Night Ambience",
            "HD Night Ambience",
        ] {
            environments.set(&mut row, column, &sound)?;
        }
        environments.rows.push(row);
        level_row[level_environment] = next_index.to_string();
        next_index += 1;
    }

    if let Ok(handle) = environments.column("Handle") {
        if let Some(row_index) = environments
            .rows
            .iter()
            .position(|row| row[handle].eq_ignore_ascii_case("ESOUNDENVIRON_INHERIT_DESECRATED"))
        {
            let assignments = [
                ("Day Event", TERROR_PROBE_SD_SOUND),
                ("HD Day Event", TERROR_PROBE_HD_SOUND),
                ("Night Event", TERROR_PROBE_SD_SOUND),
                ("HD Night Event", TERROR_PROBE_HD_SOUND),
                ("Event Delay", "25"),
                ("HD Event Delay", "25"),
            ]
            .into_iter()
            .filter_map(|(column, value)| {
                environments.column(column).ok().map(|index| (index, value))
            })
            .collect::<Vec<_>>();
            let row = &mut environments.rows[row_index];
            for (column, value) in assignments {
                row[column] = value.to_string();
            }
        }
    }
    Ok((environments, levels))
}

fn resolve_sound_filename(sounds: &TsvTable, sound_name: &str) -> Option<String> {
    let sound_column = sounds.column("Sound").ok()?;
    let filename_column = sounds.column("FileName").ok()?;
    let redirect_column = sounds.column("Redirect").ok();
    let mut current = sound_name.to_string();
    for _ in 0..8 {
        let row = sounds
            .rows
            .iter()
            .find(|row| row[sound_column].eq_ignore_ascii_case(&current))?;
        if let Some(redirect) = redirect_column
            .and_then(|index| row.get(index))
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            current = redirect.to_string();
            continue;
        }
        return Some(row[filename_column].clone());
    }
    None
}

fn collect_area_ambience_filenames(
    levels: &TsvTable,
    environments: &TsvTable,
    sounds: &TsvTable,
    areas: &[AreaCatalogEntry],
) -> Result<HashMap<u32, String>, String> {
    let level_id = levels.column("Id")?;
    let level_environment = levels.column("SoundEnv")?;
    let environment_index = environments.column("Index")?;
    let hd_day = environments.column("HD Day Ambience")?;
    let day = environments.column("Day Ambience")?;
    let hd_night = environments.column("HD Night Ambience")?;
    let night = environments.column("Night Ambience")?;
    let mut output = HashMap::new();
    for area in areas {
        let level = levels
            .rows
            .iter()
            .find(|row| row[level_id].trim() == area.area_id.to_string())
            .ok_or_else(|| format!("levels.txt 缺少 Area {}", area.area_id))?;
        let environment = environments
            .rows
            .iter()
            .find(|row| row[environment_index].trim() == level[level_environment].trim())
            .ok_or_else(|| format!("找不到 Area {} 的 SoundEnv", area.area_id))?;
        let sound_name = [hd_day, day, hd_night, night]
            .into_iter()
            .map(|column| environment[column].trim())
            .find(|value| !value.is_empty())
            .ok_or_else(|| format!("Area {} 没有持续环境音定义", area.area_id))?;
        let filename = resolve_sound_filename(sounds, sound_name).ok_or_else(|| {
            format!(
                "无法从 sounds.txt 解析 Area {} 的持续环境音 {sound_name}",
                area.area_id
            )
        })?;
        output.insert(area.area_id, filename);
    }
    Ok(output)
}

fn resolve_audio_source(
    mpq_directory: &Path,
    storage: Option<&casc_core::Storage>,
    filename: &str,
    cache_directory: &Path,
    cache_key: &str,
) -> Result<ResolvedAudioSource, String> {
    let normalized = filename.replace('\\', "/");
    let mut candidates = vec![normalized.clone()];
    if let Some(stem) = normalized.strip_suffix("_hd.flac") {
        candidates.push(format!("{stem}.flac"));
    }
    for root in [
        "data/hd/global/sfx",
        "data/hd/global/music",
        "data/global/music",
    ] {
        for candidate in &candidates {
            let local = mpq_directory.join(root).join(candidate);
            if local.is_file() {
                let is_empty = local
                    .metadata()
                    .map_err(|error| format!("读取声音资源信息失败 {}: {error}", local.display()))?
                    .len()
                    == 0;
                return Ok(ResolvedAudioSource {
                    path: (!is_empty).then_some(local),
                    label: if is_empty {
                        format!("Mod:{root}/{candidate}（空文件静音占位）")
                    } else {
                        format!("Mod:{root}/{candidate}")
                    },
                });
            }
        }
    }
    let storage = storage.ok_or_else(|| {
        format!("Mod 中找不到声音 {filename}，且没有可用的 D2R CASC；无法保留原声")
    })?;
    for root in [
        "data:data\\hd\\global\\sfx",
        "data:data\\hd\\global\\music",
        "data:data\\global\\music",
    ] {
        for candidate in &candidates {
            let internal = format!("{root}\\{}", candidate.replace('/', "\\"));
            if let Ok(bytes) = storage.read(&internal) {
                if bytes.is_empty() {
                    return Ok(ResolvedAudioSource {
                        path: None,
                        label: format!("CASC:{root}\\{candidate}（空文件静音占位）"),
                    });
                }
                let safe_key = cache_key
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                            character
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                let path = cache_directory.join(format!("{safe_key}.flac"));
                write_file(&path, bytes)?;
                return Ok(ResolvedAudioSource {
                    path: Some(path),
                    label: format!("CASC:{root}\\{candidate}"),
                });
            }
        }
    }
    Err(format!("在 Mod 与 D2R CASC 中都找不到声音资源 {filename}"))
}

fn write_marker_flac(
    path: &Path,
    marker: TelemetryMarker,
    source_audio: Option<&Path>,
    config: MarkerConfig,
) -> Result<f32, String> {
    let (mut samples, mut sample_rate, channels, bits_per_sample) =
        if let Some(source) = source_audio {
            decode_flac(source)?
        } else {
            let frames = if matches!(
                marker,
                TelemetryMarker::Area { .. } | TelemetryMarker::Frontend
            ) {
                48_000 * 5
            } else {
                48_000 / 3
            };
            (vec![0i32; frames * 2], 48_000, 2, 16)
        };
    if sample_rate < MIN_SAMPLE_RATE {
        samples = resample_interleaved_i32(&samples, channels as usize, sample_rate, 48_000);
        sample_rate = 48_000;
    }
    let periodic_location = matches!(
        marker,
        TelemetryMarker::Area { .. } | TelemetryMarker::Frontend
    ) && source_audio.is_some();
    let mut expected_detections = 1usize;
    if periodic_location {
        let interval_samples = sample_rate as usize * 5 * channels as usize;
        let mut embedded_count = 0usize;
        for chunk in samples.chunks_mut(interval_samples) {
            if chunk.len() < interval_samples {
                break;
            }
            let mut marker_chunk = chunk.to_vec();
            embed_marker(
                &mut marker_chunk,
                channels as usize,
                bits_per_sample,
                sample_rate,
                marker,
                config,
            )?;
            chunk.copy_from_slice(&marker_chunk);
            embedded_count += 1;
        }
        if embedded_count == 0 {
            embed_marker(
                &mut samples,
                channels as usize,
                bits_per_sample,
                sample_rate,
                marker,
                config,
            )?;
        } else {
            expected_detections = embedded_count;
        }
    } else {
        embed_marker(
            &mut samples,
            channels as usize,
            bits_per_sample,
            sample_rate,
            marker,
            config,
        )?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建音频目录失败 {}: {error}", parent.display()))?;
    }
    encode_flac(path, &samples, sample_rate, channels, bits_per_sample)?;
    let (verified, rate, verified_channels, verified_bits) = decode_flac(path)?;
    let mono = interleaved_i32_to_mono(&verified, verified_channels as usize, verified_bits)?;
    let detections = detect_markers(&mono, rate, config.detection_threshold)
        .into_iter()
        .filter(|detection| detection.marker == marker)
        .collect::<Vec<_>>();
    let verified = detections.len() == expected_detections;
    if !verified {
        return Err(format!(
            "生成的 {:?} FLAC 自检失败：应识别 {expected_detections} 次，实际识别 {} 次（periodic={periodic_location}）",
            marker,
            detections.len()
        ));
    }
    Ok(detections
        .iter()
        .map(|detection| detection.confidence)
        .fold(f32::INFINITY, f32::min))
}

fn write_terror_probe_flac(path: &Path) -> Result<f32, String> {
    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: usize = 2;
    const BITS_PER_SAMPLE: u32 = 16;
    let marker = TelemetryMarker::Area {
        area_id: TERROR_PROBE_MARKER_AREA_ID,
    };
    let config = MarkerConfig {
        gain_db: TERROR_MARKER_GAIN_DB,
        ..MarkerConfig::default()
    };
    let mut samples = Vec::new();
    embed_marker(
        &mut samples,
        CHANNELS,
        BITS_PER_SAMPLE,
        SAMPLE_RATE,
        marker,
        config,
    )?;

    // The desecrated event repeats for as long as the player is in a Terror
    // Zone. One compact packet per replay is sufficient to signal TZ presence
    // and avoids turning the redundant packet into a second logical heartbeat.
    let packet_gap_frames = (SAMPLE_RATE as f32 * PACKET_GAP_SECONDS).round() as usize;
    let packet_frames = (marker_frames(SAMPLE_RATE) - packet_gap_frames) / 2;
    let marker_offset_frames = (SAMPLE_RATE as f32 * MARKER_OFFSET_SECONDS).round() as usize;
    samples.truncate((marker_offset_frames + packet_frames) * CHANNELS);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建恐怖区域探针目录失败 {}: {error}", parent.display()))?;
    }
    encode_flac(
        path,
        &samples,
        SAMPLE_RATE,
        CHANNELS as u32,
        BITS_PER_SAMPLE,
    )?;
    let (verified, rate, channels, bits) = decode_flac(path)?;
    let mono = interleaved_i32_to_mono(&verified, channels as usize, bits)?;
    let detections = detect_markers(&mono, rate, config.detection_threshold)
        .into_iter()
        .filter(|detection| detection.marker == marker)
        .collect::<Vec<_>>();
    if detections.len() != 1 {
        return Err(format!(
            "恐怖区域探针自检失败：期望 1 个探针包，实际识别到 {} 个",
            detections.len()
        ));
    }
    Ok(detections[0].confidence)
}

fn default_output_parent(game_root: Option<&Path>, source: Option<&Path>) -> PathBuf {
    if let Some(game_root) = game_root {
        return game_root.join("mods");
    }
    source
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .join("D2R-Audio-Mod-Output")
}

fn asset_label(
    marker: TelemetryMarker,
    area_catalog: &[AreaCatalogEntry],
    item_catalog: &[ItemCatalogEntry],
) -> String {
    match marker {
        TelemetryMarker::Rune { rune_number } => {
            let rune = rune_data::get_rune_name(rune_number).unwrap_or("未知符文");
            format!("#{rune_number:02} {rune}")
        }
        TelemetryMarker::Item { item_id } => item_catalog
            .iter()
            .find(|item| item.item_id == item_id)
            .map(|item| format!("{} ({})", item.name, item.code))
            .unwrap_or_else(|| format!("物品 #{item_id}")),
        TelemetryMarker::Area { area_id } => area_catalog
            .iter()
            .find(|area| area.area_id == area_id)
            .map(|area| format!("{} (Area {area_id})", area.scene_name))
            .unwrap_or_else(|| format!("Area {area_id}")),
        TelemetryMarker::Frontend => "主界面".to_string(),
    }
}

pub fn build(request: BuildAudioModRequest) -> Result<BuildAudioModReport, String> {
    build_with_progress(request, |_| {})
}

pub fn build_with_progress<F>(
    request: BuildAudioModRequest,
    mut progress: F,
) -> Result<BuildAudioModReport, String>
where
    F: FnMut(BuildProgress),
{
    progress(BuildProgress::new("validate", 2, "正在检查游戏与 Mod…"));
    let tracked_categories = normalize_tracked_categories(&request.tracked_categories);
    let include_runes = tracked_categories
        .iter()
        .any(|category| category == CATEGORY_RUNES);
    let selected_items = selected_item_definitions(&tracked_categories);
    let source = non_empty_path(request.source_directory.as_deref());
    let source_layout = match request.build_mode {
        AudioModBuildMode::Minimal => None,
        AudioModBuildMode::Augment => {
            Some(find_source_layout(source.as_deref().ok_or_else(|| {
                "加工现有 Mod 模式必须选择源 Mod（建议选择其 .mpq 目录）".to_string()
            })?)?)
        }
    };
    let base_mod_name = requested_mod_name(
        request.mod_name.as_deref(),
        request.build_mode,
        source_layout.as_ref(),
    )?;
    let explicit_game_directory = non_empty_path(request.game_directory.as_deref());
    let game_root_hint = explicit_game_directory
        .as_deref()
        .into_iter()
        .chain(source.as_deref())
        .flat_map(Path::ancestors)
        .find(|candidate| is_game_storage_root(candidate))
        .map(Path::to_path_buf);
    let output_parent = request
        .output_directory
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_parent(game_root_hint.as_deref(), source.as_deref()));
    let game_root = find_game_storage_root(
        explicit_game_directory.as_deref(),
        source.as_deref(),
        &output_parent,
    );
    if request.build_mode == AudioModBuildMode::Minimal && game_root.is_none() {
        return Err("创建最小 Mod 需要有效的 D2R 游戏目录（含 .build.info 与 Data）".to_string());
    }
    progress(BuildProgress::new("game_data", 8, "正在读取游戏资源…"));
    let casc_storage_path = game_root
        .as_deref()
        .map(crate::casc_path::CascStoragePath::prepare)
        .transpose()?;
    let storage = casc_storage_path
        .as_ref()
        .map(|path| casc_core::Storage::open(path.as_path()))
        .transpose()
        .map_err(|error| {
            format!(
                "打开 D2R CASC 失败 {}: {error}",
                game_root
                    .as_deref()
                    .unwrap_or_else(|| Path::new("?"))
                    .display()
            )
        })?;
    let mod_name = available_mod_name(&output_parent, &base_mod_name);
    let final_mod_directory = output_parent.join(&mod_name);
    let staging = StagingDirectory::create(&output_parent, &mod_name)?;
    if let Some(source_mpq) = source_layout
        .as_ref()
        .and_then(|layout| layout.mpq.as_ref())
    {
        let canonical_source = std::fs::canonicalize(source_mpq)
            .map_err(|error| format!("解析源 Mod 目录失败 {}: {error}", source_mpq.display()))?;
        if staging.path.starts_with(&canonical_source) {
            return Err("输出目录不能位于源 .mpq 内部，避免递归复制".to_string());
        }
    }
    let staging_mod_directory = staging.path.clone();
    let mpq_directory = staging_mod_directory.join(format!("{mod_name}.mpq"));
    let final_mpq_directory = final_mod_directory.join(format!("{mod_name}.mpq"));
    let source_mod_copied = request.build_mode == AudioModBuildMode::Augment;
    progress(BuildProgress::new(
        "baseline",
        15,
        if source_mod_copied {
            "正在复制现有 Mod，原文件不会被修改…"
        } else {
            "正在创建纯净识别 Mod…"
        },
    ));
    let layout = match request.build_mode {
        AudioModBuildMode::Augment => {
            let layout = source_layout.ok_or_else(|| "缺少源 Mod 布局".to_string())?;
            if let Some(source_mpq) = &layout.mpq {
                copy_directory(source_mpq, &mpq_directory)?;
            } else {
                std::fs::create_dir_all(&mpq_directory)
                    .map_err(|error| format!("创建 Mod 目录失败: {error}"))?;
            }
            layout
        }
        AudioModBuildMode::Minimal => extract_minimal_baseline(
            storage
                .as_ref()
                .ok_or_else(|| "创建最小 Mod 时 D2R CASC 未打开".to_string())?,
            &mpq_directory,
        )?,
    };
    let excel_output = mpq_directory.join("data/global/excel");

    let misc_baseline =
        read_excel_with_game_fallback(&layout, storage.as_ref(), game_root.as_deref(), "misc.txt")?;
    let sounds_baseline = read_excel_with_game_fallback(
        &layout,
        storage.as_ref(),
        game_root.as_deref(),
        "sounds.txt",
    )?;
    let levels_baseline = read_excel_with_game_fallback(
        &layout,
        storage.as_ref(),
        game_root.as_deref(),
        "levels.txt",
    )?;
    let misc = TsvTable::parse("misc.txt", &misc_baseline.text)?;
    let sounds = TsvTable::parse("sounds.txt", &sounds_baseline.text)?;
    let levels = TsvTable::parse("levels.txt", &levels_baseline.text)?;
    let area_localization = load_area_localization(&mpq_directory, storage.as_ref())?;
    let mut areas = collect_areas_localized(&levels, &area_localization)?;
    if request.area_coverage == AudioAreaCoverage::CountessRoute {
        areas.retain(|area| COUNTESS_AREA_IDS.contains(&area.area_id));
        if areas.len() != COUNTESS_AREA_IDS.len() {
            return Err("女伯爵路线需要 levels.txt 包含 Area 1、6、20–25".to_string());
        }
    } else if areas.len() < 100 {
        return Err(format!(
            "全区域模式只从 levels.txt 解析到 {} 个有效区域，疑似不是完整游戏数据",
            areas.len()
        ));
    }

    let explicit_sound_environment = request
        .sound_environment_file
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    let sound_environment_baseline = if let Some(path) = explicit_sound_environment {
        ResolvedTextFile {
            text: read_excel_text(&path)?,
            source: path.to_string_lossy().into_owned(),
            from_source_mod: true,
        }
    } else {
        read_excel_with_game_fallback(
            &layout,
            storage.as_ref(),
            game_root.as_deref(),
            "soundenviron.txt",
        )?
    };
    let environments = TsvTable::parse("soundenviron.txt", &sound_environment_baseline.text)?;
    let area_ambience_filenames =
        collect_area_ambience_filenames(&levels, &environments, &sounds, &areas)?;
    progress(BuildProgress::new("areas", 28, "正在准备全部场景声纹…"));
    let casc_cache = staging_mod_directory.join(".audio-telemetry-casc-cache");
    validate_misc(&misc, include_runes, &selected_items)?;
    let mut compatibility = vec![AudioModCompatibility {
        target: if request.build_mode == AudioModBuildMode::Minimal {
            "Mod 基线".to_string()
        } else {
            "现有 Mod".to_string()
        },
        action: if request.build_mode == AudioModBuildMode::Minimal {
            "create_from_game".to_string()
        } else {
            "copy_without_overwrite".to_string()
        },
        detail: if request.build_mode == AudioModBuildMode::Minimal {
            format!(
                "从 {} 的本机 CASC 提取必要表格与所选世界物品实体；不依赖第三方 Mod。",
                game_root
                    .as_deref()
                    .unwrap_or_else(|| Path::new("?"))
                    .display()
            )
        } else {
            "源 Mod 未被修改；加工结果写入新的组合 Mod 目录。".to_string()
        },
    }];
    let room_tools_installed =
        install_in_game_room_tools(&mpq_directory, storage.as_ref(), &mut compatibility)?;
    if request.build_mode == AudioModBuildMode::Augment {
        for (name, baseline) in [
            ("misc.txt", &misc_baseline),
            ("sounds.txt", &sounds_baseline),
            ("levels.txt", &levels_baseline),
            ("soundenviron.txt", &sound_environment_baseline),
        ] {
            if !baseline.from_source_mod {
                compatibility.push(AudioModCompatibility {
                    target: format!("数据表 {name}"),
                    action: "use_game_baseline".to_string(),
                    detail: format!(
                        "源 Mod 未提供 {name}，已从本机 D2R 游戏数据补齐；源 Mod 中其他文件保持不变。"
                    ),
                });
            }
        }
    }
    let rune_plans = if include_runes {
        patch_rune_unit_definitions(&mpq_directory, storage.as_ref(), &mut compatibility)?
    } else {
        Vec::new()
    };
    let (mut items_document, items_source) = if selected_items.is_empty() {
        (serde_json::json!([]), "未选择扩展物品".to_string())
    } else {
        read_json_asset(&mpq_directory, storage.as_ref(), "data/hd/items/items.json")?
    };
    let baseline_items_document = if selected_items.is_empty() {
        None
    } else {
        storage
            .as_ref()
            .map(|storage| read_casc_json_asset(storage, "data/hd/items/items.json"))
            .transpose()?
    };
    let item_localization = if selected_items.is_empty() {
        HashMap::new()
    } else {
        match load_item_localization(&mpq_directory, storage.as_ref()) {
            Ok(localization) => localization,
            Err(error) => {
                compatibility.push(AudioModCompatibility {
                    target: "扩展物品名称".to_string(),
                    action: "use_builtin_fallback".to_string(),
                    detail: format!(
                        "未找到可读取的游戏物品名称表，将使用协议内置中英文名称；不影响声纹资源加工。详情：{error}"
                    ),
                });
                HashMap::new()
            }
        }
    };
    let item_plans = patch_item_unit_definitions(
        &mpq_directory,
        storage.as_ref(),
        &mut items_document,
        baseline_items_document.as_ref(),
        &selected_items,
        &item_localization,
        &mut compatibility,
    )?;
    progress(BuildProgress::new(
        "items",
        42,
        "正在保留物品模型并附加掉落声纹…",
    ));
    let item_catalog_entries = item_plans
        .iter()
        .map(|plan| plan.entry.clone())
        .collect::<Vec<_>>();
    let (sounds, definitions) = patch_sounds(
        sounds,
        &rune_plans,
        &item_plans,
        &areas,
        &area_ambience_filenames,
        &mut compatibility,
    )?;
    let (environments, levels) = patch_sound_environ_and_levels(environments, levels, &areas)?;

    write_file(&excel_output.join("misc.txt"), misc.to_text())?;
    write_file(&excel_output.join("sounds.txt"), sounds.to_text())?;
    write_file(&excel_output.join("levels.txt"), levels.to_text())?;
    write_file(
        &excel_output.join("soundenviron.txt"),
        environments.to_text(),
    )?;
    if !selected_items.is_empty() {
        write_file(
            &mpq_directory.join("data/hd/items/items.json"),
            serde_json::to_vec_pretty(&items_document)
                .map_err(|error| format!("序列化 items.json 失败: {error}"))?,
        )?;
    }
    let config = MarkerConfig {
        gain_db: request.gain_db.unwrap_or(MarkerConfig::default().gain_db),
        ..MarkerConfig::default()
    }
    .validate()?;
    let mut rune_assets = Vec::new();
    let mut item_assets = Vec::new();
    let mut area_assets = Vec::new();
    let mut terror_assets = Vec::new();
    let mut frontend_assets = Vec::new();
    let definition_count = definitions.len().max(1);
    for (definition_index, definition) in definitions.into_iter().enumerate() {
        if definition_index == 0 || definition_index % 16 == 0 {
            let percent = 46 + ((definition_index * 42) / definition_count) as u8;
            progress(BuildProgress::new(
                "audio",
                percent,
                format!(
                    "正在加工声纹资源（{}/{definition_count}）…",
                    definition_index + 1
                ),
            ));
        }
        let marker = definition.marker;
        let group = definition.group;
        let resolved_source = definition
            .source_filename
            .as_deref()
            .map(|filename| {
                resolve_audio_source(
                    &mpq_directory,
                    storage.as_ref(),
                    filename,
                    &casc_cache,
                    &format!("{}-{}", marker_sort_key(marker), definition.sound),
                )
            })
            .transpose()?;
        let source_audio = resolved_source
            .as_ref()
            .and_then(|source| source.path.as_deref());
        let output_path = mpq_directory
            .join(definition.output_root)
            .join(definition.relative_path.replace('\\', "/"));
        let marker_config = if group == SoundAssetGroup::Terror {
            MarkerConfig {
                gain_db: TERROR_MARKER_GAIN_DB,
                ..config
            }
        } else {
            config
        };
        let confidence = write_marker_flac(&output_path, marker, source_audio, marker_config)?;
        let asset = AudioModAsset {
            marker,
            label: match group {
                SoundAssetGroup::Terror => "恐怖区域即时入场 · TZ 1023".to_string(),
                SoundAssetGroup::Frontend => format!("主界面 · {}", definition.sound),
                SoundAssetGroup::Rune | SoundAssetGroup::Item | SoundAssetGroup::Area => {
                    asset_label(marker, &areas, &item_catalog_entries)
                }
            },
            sound: definition.sound,
            relative_path: definition.relative_path,
            source_audio: resolved_source.as_ref().map(|source| source.label.clone()),
            preserved_source_audio: source_audio.is_some(),
            confidence,
        };
        match group {
            SoundAssetGroup::Rune => rune_assets.push(asset),
            SoundAssetGroup::Item => item_assets.push(asset),
            SoundAssetGroup::Area => area_assets.push(asset),
            SoundAssetGroup::Terror => terror_assets.push(asset),
            SoundAssetGroup::Frontend => frontend_assets.push(asset),
        }
    }
    let terror_probe_path = mpq_directory
        .join("data/hd/global/sfx")
        .join(TERROR_PROBE_RELATIVE_PATH.replace('\\', "/"));
    let terror_probe_confidence = write_terror_probe_flac(&terror_probe_path)?;
    terror_assets.push(AudioModAsset {
        marker: TelemetryMarker::Area {
            area_id: TERROR_PROBE_MARKER_AREA_ID,
        },
        label: "恐怖区域延迟兜底 · TZ 1023".to_string(),
        sound: TERROR_PROBE_HD_SOUND.to_string(),
        relative_path: TERROR_PROBE_RELATIVE_PATH.to_string(),
        source_audio: None,
        preserved_source_audio: false,
        confidence: terror_probe_confidence,
    });
    compatibility.push(AudioModCompatibility {
        target: "恐怖区域状态识别".to_string(),
        action: "emit_shared_terror_zone_marker".to_string(),
        detail: format!(
            "保留恐怖区域音乐与持续环境声；所有恐怖区域共用同一个短促声纹，只表示当前处于 TZ，不再编码具体 Area。即时入场与延迟兜底为保证 3D 混音后的可靠性固定使用 {:.0} dBFS（兜底本地自检置信度 {:.1}%）。",
            TERROR_MARKER_GAIN_DB,
            terror_probe_confidence * 100.0
        ),
    });
    if casc_cache.is_dir() {
        std::fs::remove_dir_all(&casc_cache).map_err(|error| {
            format!("清理 CASC 环境音缓存失败 {}: {error}", casc_cache.display())
        })?;
    }

    progress(BuildProgress::new(
        "catalogs",
        90,
        "正在生成识别清单并自检…",
    ));

    let modinfo_path = mpq_directory.join("modinfo.json");
    let mut modinfo = if modinfo_path.is_file() {
        parse_json_value(&read_utf8(&modinfo_path)?)
            .map_err(|error| format!("解析源 Mod modinfo.json 失败: {error}"))?
    } else {
        serde_json::json!({})
    };
    let modinfo_object = modinfo
        .as_object_mut()
        .ok_or_else(|| "源 Mod modinfo.json 必须是 JSON 对象".to_string())?;
    modinfo_object.insert(
        "name".to_string(),
        serde_json::Value::String(mod_name.clone()),
    );
    if request.build_mode == AudioModBuildMode::Minimal {
        modinfo_object.insert(
            "author".to_string(),
            serde_json::Value::String("D2R Audio Telemetry".to_string()),
        );
    }
    modinfo_object
        .entry("savepath".to_string())
        .or_insert_with(|| serde_json::Value::String("../".to_string()));
    modinfo_object.insert(
        "audio_telemetry_protocol".to_string(),
        serde_json::json!(PROTOCOL_VERSION),
    );
    write_file(
        &modinfo_path,
        serde_json::to_vec_pretty(&modinfo)
            .map_err(|error| format!("生成 modinfo.json 失败: {error}"))?,
    )?;
    let catalog_file = AreaCatalogFile {
        protocol_version: PROTOCOL_VERSION,
        source_levels: if request.build_mode == AudioModBuildMode::Minimal {
            format!(
                "CASC:{}:data/global/excel/levels.txt",
                game_root
                    .as_deref()
                    .unwrap_or_else(|| Path::new("?"))
                    .display()
            )
        } else {
            levels_baseline.source.clone()
        },
        areas: areas.clone(),
    };
    let catalog_json = serde_json::to_vec_pretty(&catalog_file)
        .map_err(|error| format!("生成地图声纹目录失败: {error}"))?;
    write_file(
        &staging_mod_directory.join(AREA_CATALOG_FILE_NAME),
        &catalog_json,
    )?;
    let item_catalog_file = build_item_catalog_file(items_source, item_catalog_entries.clone());
    let item_catalog_json = serde_json::to_vec_pretty(&item_catalog_file)
        .map_err(|error| format!("生成物品声纹目录失败: {error}"))?;
    write_file(
        &staging_mod_directory.join(ITEM_CATALOG_FILE_NAME),
        &item_catalog_json,
    )?;

    let area_count = areas.len();
    let report = BuildAudioModReport {
        manifest_format: "d2r-audio-telemetry-mod".to_string(),
        producer: "d2r-audio-mod".to_string(),
        producer_version: env!("CARGO_PKG_VERSION").to_string(),
        recipe_version: AUDIO_MOD_RECIPE_VERSION,
        capabilities: if room_tools_installed {
            vec![
                TERROR_IMMEDIATE_ENTRY_CAPABILITY.to_string(),
                IN_GAME_ROOM_TOOLS_CAPABILITY.to_string(),
            ]
        } else {
            vec![TERROR_IMMEDIATE_ENTRY_CAPABILITY.to_string()]
        },
        generated_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        protocol_version: PROTOCOL_VERSION,
        build_mode: request.build_mode,
        area_coverage: request.area_coverage,
        mod_name: mod_name.clone(),
        mod_directory: final_mod_directory.to_string_lossy().to_string(),
        mpq_directory: final_mpq_directory.to_string_lossy().to_string(),
        source_excel_directory: if request.build_mode == AudioModBuildMode::Minimal {
            format!(
                "CASC:{}:data/global/excel",
                game_root.as_deref().unwrap_or_else(|| Path::new("?" )).display()
            )
        } else if misc_baseline.from_source_mod
            && sounds_baseline.from_source_mod
            && levels_baseline.from_source_mod
        {
            layout.excel.to_string_lossy().into_owned()
        } else {
            format!(
                "mixed:misc.txt={};sounds.txt={};levels.txt={}",
                misc_baseline.source, sounds_baseline.source, levels_baseline.source
            )
        },
        source_mod_copied,
        source_mod_name: if request.build_mode == AudioModBuildMode::Augment {
            inferred_source_mod_name(request.source_directory.as_deref())
        } else {
            None
        },
        sound_environment_source: sound_environment_baseline.source,
        launch_arguments: format!("-mod {mod_name} -txt"),
        rune_assets,
        item_assets,
        area_assets,
        terror_assets,
        frontend_assets,
        area_catalog: areas,
        compatibility,
        notes: vec![
            format!(
                "已按选择加工 {} 个符文与 {} 个扩展物品；每个目标均克隆原实体与 Flippy ↔ Ground 状态机，并同步复制其普通/低配背包 sprite，在独立 items.json 映射下保留原模型、物品图标、动画、VFX、依赖和转场。",
                if include_runes { RUNE_COUNT } else { 0 },
                item_catalog_entries.len()
            ),
            "misc.txt 的 dropsound 与 usesound 均保持原值；背包/仓库不进入地面状态。"
                .to_string(),
            "主界面使用 music_options、五幕前端场景、营火、选角循环与稳定 event_fe_act_* 条目的独立混音文件；music_desecrated_hd 及其恐惧区域资源保持不变。"
                .to_string(),
            if request.area_coverage == AudioAreaCoverage::AllAreas {
                format!(
                    "已覆盖 levels.txt 中的全部 {} 个有效区域；环境原声优先取现有 Mod，缺失时取本机 D2R CASC。",
                    area_count
                )
            } else {
                format!(
                    "当前覆盖女伯爵路线 Area {:?}；环境原声优先取现有 Mod，缺失时取本机 D2R CASC。",
                    COUNTESS_AREA_IDS
                )
            },
            "v7 使用独立地点/掉落同步码与 127 路 Gold 掉落签名；主界面标记会立即结束未完成的刷图计时。"
                .to_string(),
            if room_tools_installed {
                "局内顶部工具栏可在确认后开始下一局，或打开原生创建/加入房间面板；房名、密码与描述输入框沿用局内聊天框的键盘独占规则，不再触发技能快捷键，表单支持同键、Esc 与关闭按钮退出，并继续使用 D2R 自带的缓存与回车提交。".to_string()
            } else {
                "本次没有可安全复用的完整游戏 UI 布局，未追加局内房间工具。".to_string()
            },
        ],
    };
    write_file(
        &staging_mod_directory.join("audio-telemetry-manifest.json"),
        serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("生成音频 Mod 清单失败: {error}"))?,
    )?;
    write_file(
        &staging_mod_directory.join("README-安装与测试.txt"),
        format!(
            "D2R 音频遥测 Mod 工具\r\n\r\n启动参数：{}\r\n\r\n1. 输出目录是独立组合 Mod，源 Mod 没有被修改。\r\n2. 在你的启动器中启用上面的 -mod/-txt；本工具不会修改账号或启动器配置。\r\n3. Mod 只播放 v7 协议声纹；接收、统计由兼容软件独立完成。\r\n4. 所选掉落只加工世界实体的 Flippy 音频入口，背包/仓库 usesound 与 misc.txt dropsound 保持原值。\r\n5. 原实体与状态机从源 Mod 或本机游戏克隆，并同步复制其普通/低配背包 sprite；原模型、物品图标、动画、VFX、依赖和转场保留，原入口已有声音时保留原声并混入声纹。\r\n6. 主界面条目使用独立文件，不修改恐惧区域复用的 options_hd.flac。\r\n7. 本次地图覆盖：{}；掉落覆盖：{} 个符文、{} 个扩展物品。\r\n8. 游戏“音效”通道必须非静音；若仅依赖主界面音乐兜底，音乐通道也不能完全静音。\r\n9. 声纹只能区分基础物品代码，不能区分共享同一代码的词缀、品质或鉴定结果。\r\n10. {}\r\n",
            report.launch_arguments,
            if report.area_coverage == AudioAreaCoverage::AllAreas { "全部区域" } else { "女伯爵路线" },
            report.rune_assets.len(),
            report.item_assets.len(),
            if room_tools_installed {
                "进入在线游戏后，顶部工具栏可在二次确认后开始下一局；创建/加入面板会像聊天框一样优先接收房名、密码与描述的键盘输入，并可用同一按钮、Esc 或面板关闭按钮退出。"
            } else {
                "本次未追加局内房间工具；加工现有 Mod 时请同时提供有效的 D2R 游戏目录。"
            }
        ),
    )?;
    progress(BuildProgress::new("finish", 98, "正在完成 Mod…"));
    staging.commit(&final_mod_directory)?;
    progress(BuildProgress::new("complete", 100, "Mod 已准备完成"));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_room_tools_without_replacing_existing_hud_content() {
        let root =
            std::env::temp_dir().join(format!("d2rhub-room-tools-test-{}", uuid::Uuid::new_v4()));
        let layouts = root.join(UI_LAYOUTS_DIRECTORY);
        write_file(
            &layouts.join("HudWarningshd.json"),
            br#"{
                type: 'HUDWarningsPanel',
                name: 'HUDWarnings',
                fields: { priority: -100 },
                children: [
                    { type: 'TimerWidget', name: 'ExistingLauncher', fields: { time: 0.1, message: 'PanelManager:OpenPanel:ExistingPanel' } },
                ],
            }"#,
        )
        .unwrap();
        write_file(
            &layouts.join("creategamepanelhd.json"),
            br#"{
                type: 'CreateGamePanel',
                name: 'CreateGamePanel',
                fields: { anchor: '$LobbyAnchor', defaultWidget: 'GameNameInput' },
                children: [
                    {
                        type: 'TextBoxWidget',
                        name: 'CreateFields',
                        children: [
                            { type: 'TextBoxWidget', name: 'GameNameInput', fields: { imeEnabled: true } },
                            { type: 'TextBoxWidget', name: 'PasswordInput', fields: { imeEnabled: true } },
                            { type: 'TextBoxWidget', name: 'DescriptionInput', fields: { imeEnabled: true } },
                        ],
                    },
                ],
            }"#,
        )
        .unwrap();
        write_file(
            &layouts.join("joingamepanelhd.json"),
            br#"{
                type: 'JoinGamePanel',
                name: 'JoinGamePanel',
                fields: { anchor: '$LobbyAnchor', defaultWidget: 'NameInput' },
                children: [
                    { type: 'TextBoxWidget', name: 'NameInput', fields: { imeEnabled: true } },
                    { type: 'TextBoxWidget', name: 'PasswordInput', fields: { imeEnabled: true } },
                    {
                        type: 'ButtonWidget',
                        name: 'JoinButton',
                        fields: { rect: { x: -3, y: 1275 }, filename: 'Lobby\\Final\\LobbyButton' },
                    },
                ],
            }"#,
        )
        .unwrap();

        let mut compatibility = Vec::new();
        install_in_game_room_tools(&root, None, &mut compatibility).unwrap();
        install_in_game_room_tools(&root, None, &mut compatibility).unwrap();

        let hud =
            parse_json_value(&read_utf8(&layouts.join("HudWarningshd.json")).unwrap()).unwrap();
        let children = hud["children"].as_array().unwrap();
        assert!(children
            .iter()
            .any(|child| { child["fields"]["message"] == "PanelManager:OpenPanel:ExistingPanel" }));
        assert_eq!(
            children
                .iter()
                .filter(|child| {
                    child["fields"]["message"] == "PanelManager:OpenPanel:D2RHubRoomToolbar"
                })
                .count(),
            1
        );

        let toolbar =
            parse_json_value(&read_utf8(&layouts.join("D2RHubRoomToolbarhd.json")).unwrap())
                .unwrap();
        let names = toolbar["children"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|child| child["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["D2RHubNextGame", "D2RHubCreateGame", "D2RHubJoinGame"]
        );
        assert_eq!(
            find_layout_node(&quick_recreate_layout(), "D2RHubQuickRecreateAction").unwrap()
                ["fields"]["message"],
            "CharacterSelect:LoadCharacter:2"
        );
        assert_eq!(
            find_layout_node(&toolbar, "D2RHubNextGame").unwrap()["fields"]["onClickMessage"],
            "PanelManager:TogglePanel:D2RHubQuickRecreateConfirm"
        );
        let next_game = find_layout_node(&toolbar, "D2RHubNextGame").unwrap();
        assert!(next_game["fields"]["tooltipOffset"].is_null());
        assert!(!next_game["fields"]["tooltipString"]
            .as_str()
            .unwrap()
            .starts_with('@'));
        let confirmation = parse_json_value(
            &read_utf8(&layouts.join("D2RHubQuickRecreateConfirmhd.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(confirmation["fields"]["isDismissable"], true);
        assert_eq!(
            find_layout_node(&confirmation, "D2RHubConfirmNextGame").unwrap()["fields"]
                ["onClickMessage"],
            "PanelManager:OpenPanel:D2RHubQuickRecreate"
        );

        for (helper_name, target) in [
            ("D2RHubOpenCreateGamehd.json", "CreateGamePanel"),
            ("D2RHubOpenJoinGamehd.json", "JoinGamePanel"),
        ] {
            let helper = parse_json_value(&read_utf8(&layouts.join(helper_name)).unwrap()).unwrap();
            assert!(helper.get("fields").is_none());
            assert_eq!(
                find_layout_node(&helper, "D2RHubOpenRoomPanel").unwrap()["fields"]["message"],
                format!("PanelManager:TogglePanel:{target}")
            );
            for child in helper["children"].as_array().unwrap() {
                assert_eq!(child["fields"]["time"], 0.1);
            }
        }

        let create =
            parse_json_value(&read_utf8(&layouts.join("creategamepanelhd.json")).unwrap()).unwrap();
        assert_eq!(create["fields"]["priority"], 2);
        assert!(create["fields"].get("defaultWidget").is_none());
        assert_eq!(create["fields"]["isDismissable"], true);
        assert_eq!(create["fields"]["acceptsEscKeyEverywhere"], true);
        for input_name in ["GameNameInput", "PasswordInput", "DescriptionInput"] {
            let input = find_layout_node(&create, input_name).unwrap();
            assert_eq!(input["fields"]["imeEnabled"], true);
            assert_eq!(input["fields"]["alwaysAcceptsKeyInput"], true);
        }
        assert_eq!(
            find_layout_node(&create, "D2RHubCloseRoomForm").unwrap()["fields"]["onClickMessage"],
            "PanelManager:ClosePanel:CreateGamePanel"
        );
        let join =
            parse_json_value(&read_utf8(&layouts.join("joingamepanelhd.json")).unwrap()).unwrap();
        assert_eq!(join["fields"]["priority"], 2);
        assert!(join["fields"].get("defaultWidget").is_none());
        assert_eq!(join["fields"]["isDismissable"], true);
        assert_eq!(join["fields"]["acceptsEscKeyEverywhere"], true);
        for input_name in ["NameInput", "PasswordInput"] {
            let input = find_layout_node(&join, input_name).unwrap();
            assert_eq!(input["fields"]["imeEnabled"], true);
            assert_eq!(input["fields"]["alwaysAcceptsKeyInput"], true);
        }
        assert_eq!(
            find_layout_node(&join, "JoinButton").unwrap()["fields"]["rect"]["x"],
            330
        );
        assert_eq!(
            find_layout_node(&join, "JoinButton").unwrap()["fields"]["rect"]["y"],
            1080
        );
        assert_eq!(
            find_layout_node(&join, "D2RHubCloseRoomForm").unwrap()["fields"]["onClickMessage"],
            "PanelManager:ClosePanel:JoinGamePanel"
        );
        assert_eq!(
            compatibility.last().unwrap().action,
            "add_in_game_create_join_and_recreate"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_a_stable_source_mod_hint() {
        assert_eq!(
            inferred_source_mod_name(Some(r"C:\Games\D2R\mods\jcy")),
            Some("jcy".to_string())
        );
        assert_eq!(
            inferred_source_mod_name(Some(r"C:\Games\D2R\mods\jcy\jcy.mpq")),
            Some("jcy".to_string())
        );
        assert_eq!(inferred_source_mod_name(None), None);
    }

    #[test]
    fn validates_explicit_mod_names_without_rewriting_them() {
        assert_eq!(
            requested_mod_name(Some("MyAudio-Mod_2"), AudioModBuildMode::Minimal, None).unwrap(),
            "MyAudio-Mod_2"
        );
        for invalid in ["", "bad name", "../escape", "中文名", "CON", "COM1"] {
            assert!(
                requested_mod_name(Some(invalid), AudioModBuildMode::Minimal, None).is_err(),
                "unexpectedly accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn accepts_commented_json5_area_localization() {
        let root = std::env::temp_dir().join(format!(
            "d2rhub-audio-level-localization-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join("data/local/lng/strings/levels.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "\u{feff}// levels with map level\n[{ Key: 'Black Marsh', enUS: 'Black Marsh', zhCN: '黑色荒地', },]",
        )
        .unwrap();

        let localization = load_area_localization(&root, None).unwrap();
        assert_eq!(
            localization.get("black marsh"),
            Some(&("黑色荒地".to_string(), "Black Marsh".to_string()))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn treats_an_empty_mod_flac_as_silence_without_restoring_game_audio() {
        let root =
            std::env::temp_dir().join(format!("d2rhub-audio-empty-flac-{}", uuid::Uuid::new_v4()));
        let source = root.join("data/hd/global/sfx/ambient/scene.flac");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, []).unwrap();

        let resolved = resolve_audio_source(
            &root,
            None,
            "ambient\\scene.flac",
            &root.join("cache"),
            "area-6",
        )
        .unwrap();
        assert!(resolved.path.is_none());
        assert!(resolved.label.contains("空文件静音占位"));

        let tagged = root.join("tagged.flac");
        write_marker_flac(
            &tagged,
            TelemetryMarker::Area { area_id: 6 },
            resolved.path.as_deref(),
            MarkerConfig::default(),
        )
        .unwrap();
        let (samples, sample_rate, channels, _) = decode_flac(&tagged).unwrap();
        assert_eq!(samples.len(), sample_rate as usize * channels as usize * 5);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_an_outer_mod_directory_with_partial_excel_overrides() {
        let root =
            std::env::temp_dir().join(format!("d2rhub-audio-layout-{}", uuid::Uuid::new_v4()));
        let outer = root.join("hongye");
        let mpq = outer.join("hongye.mpq");
        let excel = mpq.join("data/global/excel");
        std::fs::create_dir_all(&excel).unwrap();
        std::fs::write(excel.join("misc.txt"), "name\tcode\n").unwrap();

        let layout = find_source_layout(&outer).unwrap();
        assert_eq!(layout.mpq.as_deref(), Some(mpq.as_path()));
        assert_eq!(layout.excel, excel);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_each_table_from_the_mod_or_fallback_independently() {
        let root =
            std::env::temp_dir().join(format!("d2rhub-audio-table-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let local = root.join("misc.txt");
        std::fs::write(&local, "mod-version").unwrap();

        let local_result = read_text_with_fallback(&local, "CASC:misc.txt".to_string(), || {
            panic!("fallback must not run when the Mod provides the table")
        })
        .unwrap();
        assert_eq!(local_result.text, "mod-version");
        assert!(local_result.from_source_mod);

        let missing = root.join("levels.txt");
        let fallback_result =
            read_text_with_fallback(&missing, "CASC:levels.txt".to_string(), || {
                Ok("game-version".to_string())
            })
            .unwrap();
        assert_eq!(fallback_result.text, "game-version");
        assert_eq!(fallback_result.source, "CASC:levels.txt");
        assert!(!fallback_result.from_source_mod);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn decodes_common_source_mod_table_encodings() {
        assert_eq!(
            decode_excel_text(b"\xef\xbb\xbfName\tCode\n").unwrap(),
            "Name\tCode\n"
        );

        let utf16_le = [
            0xff, 0xfe, b'N', 0, b'a', 0, b'm', 0, b'e', 0, b'\t', 0, b'C', 0, b'o', 0, b'd', 0,
            b'e', 0, b'\n', 0,
        ];
        assert_eq!(decode_excel_text(&utf16_le).unwrap(), "Name\tCode\n");

        let utf16_be_without_bom = [
            0, b'N', 0, b'a', 0, b'm', 0, b'e', 0, b'\t', 0, b'C', 0, b'o', 0, b'd', 0, b'e', 0,
            b'\n',
        ];
        assert_eq!(
            decode_excel_text(&utf16_be_without_bom).unwrap(),
            "Name\tCode\n"
        );

        let gbk = [
            0xc3, 0xfb, 0xb3, 0xc6, b'\t', 0xb4, 0xfa, 0xc2, 0xeb, b'\n', 0xb7, 0xfb, 0xce, 0xc4,
            b'\t', 0xff, b'c', b'1', b'1', b'\n',
        ];
        assert_eq!(decode_excel_text(&gbk).unwrap(), "名称\t代码\n符文\tÿc11\n");

        assert_eq!(
            decode_excel_text(b"name\tcode\n\xffc1Unique\tabc\n").unwrap(),
            "name\tcode\nÿc1Unique\tabc\n"
        );
    }

    fn write_rune_unit_definitions(mpq: &Path) {
        let directory = mpq.join("data/hd/items/misc/rune");
        std::fs::create_dir_all(&directory).unwrap();
        let state_machine_path =
            mpq.join("data/hd/items/dropped_items/dropped_items_helms_flip_ne.json");
        std::fs::create_dir_all(state_machine_path.parent().unwrap()).unwrap();
        let state_machine = serde_json::json!({
            "dependencies": {
                "particles": [{ "path": "data/hd/vfx2/custom-preserved.particles" }],
                "models": [],
                "skeletons": [],
                "animations": [{ "path": "data/hd/items/dropped_items/animation/dropped_items_helms_flip_ne.animation" }],
                "textures": [],
                "physics": [],
                "json": [],
                "variantdata": [],
                "objecteffects": [],
                "other": []
            },
            "type": "AnimationStateMachine",
            "name": "dropped_items_helms_flip_ne",
            "unitType": "UNIT_OBJECT",
            "animations": [{
                "type": "AnimationItem",
                "name": "preserved_animation",
                "filename": "data/hd/items/dropped_items/animation/dropped_items_helms_flip_ne.animation"
            }],
            "states": [{
                "type": "AnimationState",
                "name": "AnimationState",
                "_name": "Flippy",
                "audioId": "",
                "stateId": 1,
                "customPreservedField": 42
            }, {
                "type": "AnimationState",
                "name": "AnimationState001",
                "_name": "Ground",
                "audioId": "",
                "stateId": 2
            }],
            "transitions": [{
                "from": 1,
                "settings": [{ "to": 2, "crossfadeSeconds": 0.2 }]
            }, {
                "from": 2,
                "settings": [{ "to": 1, "crossfadeSeconds": 0.2 }]
            }]
        });
        std::fs::write(
            state_machine_path,
            serde_json::to_vec_pretty(&state_machine).unwrap(),
        )
        .unwrap();
        for (index, name) in rune_data::RUNE_NAMES_EN.iter().enumerate() {
            let document = serde_json::json!({
                "dependencies": {
                    "json": [{
                        "path": "data/hd/items/dropped_items/dropped_items_helms_flip_ne.json"
                    }]
                },
                "type": "UnitDefinition",
                "name": format!("{}_rune", name.to_ascii_lowercase()),
                "entities": [{
                    "type": "Entity",
                    "name": "entity_root",
                    "id": 1000 + index,
                    "components": [{
                        "type": "UnitRootComponent",
                        "name": "component_root",
                        "state_machine_filename": "data/hd/items/dropped_items/dropped_items_helms_flip_ne.json"
                    }]
                }]
            });
            std::fs::write(
                directory.join(format!("{}_rune.json", name.to_ascii_lowercase())),
                serde_json::to_vec_pretty(&document).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn patches_all_runes_and_countess_area_rows() {
        let mut misc = "name\tcode\tdropsound\tusesound\n".to_string();
        for number in 1..=33 {
            misc.push_str(&format!(
                "Rune {number}\tr{number:02}\titem_rune\titem_rune\n"
            ));
        }
        let original = TsvTable::parse("misc", &misc).unwrap();
        validate_misc(&original, true, &[]).unwrap();
        assert_eq!(
            original.get(&original.rows[0], "dropsound"),
            Some("item_rune")
        );
        assert_eq!(
            original.get(&original.rows[32], "dropsound"),
            Some("item_rune")
        );
        assert_eq!(
            original.get(&original.rows[0], "usesound"),
            Some("item_rune")
        );
        assert_eq!(
            original.get(&original.rows[32], "usesound"),
            Some("item_rune")
        );

        let levels = TsvTable::parse(
            "levels",
            "Name\tId\tSoundEnv\tLevelName\nNull\t0\t0\tNull\nAct 1 - Town\t1\t1\tRogue Encampment\nAct 1 - Wilderness 5\t6\t2\tBlack Marsh\nAct 1 - Crypt 1\t20\t2\tForgotten Tower\nAct 1 - Crypt 2\t21\t2\tTower Cellar Level 1\nAct 1 - Crypt 3\t22\t2\tTower Cellar Level 2\nAct 1 - Crypt 4\t23\t2\tTower Cellar Level 3\nAct 1 - Crypt 5\t24\t2\tTower Cellar Level 4\nAct 1 - Crypt 6\t25\t2\tTower Cellar Level 5\n",
        )
        .unwrap();
        let areas = collect_areas(&levels).unwrap();
        assert_eq!(areas.len(), 8);
        assert_eq!(areas[0].kind, LocationKind::Town);
        assert_eq!(areas[1].scene_name, "Black Marsh");
        let environments = TsvTable::parse(
            "soundenviron",
            "Handle\tIndex\tDay Ambience\tHD Day Ambience\tNight Ambience\tHD Night Ambience\tDay Event\tHD Day Event\tNight Event\tHD Night Event\tEvent Delay\tHD Event Delay\nTown\t1\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\ta\ta\ta\ta\t500\t500\nWild\t2\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tb\tb\tb\tb\t500\t500\n",
        )
        .unwrap();
        let (environments, levels) =
            patch_sound_environ_and_levels(environments, levels, &areas).unwrap();
        assert_eq!(environments.rows.len(), 10);
        assert_eq!(levels.get(&levels.rows[1], "SoundEnv"), Some("3"));
        assert_eq!(levels.get(&levels.rows[2], "SoundEnv"), Some("4"));

        let sounds = TsvTable::parse(
            "sounds",
            "Sound\t*Index\tRedirect\tFileName\tIsAmbientScene\tIsAmbientEvent\tGroup Weight\tLoop\tHDOptOut\nitem_rune_hd\t10\t\titem\\rune.flac\t0\t0\t0\t0\t0\nscene_wilderness_day\t11\t\tambient\\scene.flac\t1\t0\t0\t1\t0\nmusic_options\t12\t\tcommon\\options.flac\t0\t0\t0\t1\t0\nact1_scene_front_end\t13\t\tfrontend\\act1.flac\t1\t0\t0\t1\t0\ncampfire_front_end\t14\t\tfrontend\\campfire.flac\t1\t0\t0\t1\t0\nchar_select_fe_fire_loop_hd\t15\t\tfrontend\\fire.flac\t1\t0\t0\t1\t0\ndesecrated_enter\t16\tdesecrated_enter_hd\tquest\\desecrated_enter.flac\t0\t0\t0\t0\t0\ndesecrated_enter_hd\t17\t\tquest\\desecrated_enter_hd.flac\t0\t0\t0\t0\t0\n",
        )
        .unwrap();
        let rune_plans = (1..=RUNE_COUNT)
            .map(|rune_number| RuneStatePlan {
                rune_number,
                original_audio_id: None,
            })
            .collect::<Vec<_>>();
        let area_filenames = areas
            .iter()
            .map(|area| (area.area_id, "ambient\\scene.flac".to_string()))
            .collect::<HashMap<_, _>>();
        let mut compatibility = Vec::new();
        let (sounds, definitions) = patch_sounds(
            sounds,
            &rune_plans,
            &[],
            &areas,
            &area_filenames,
            &mut compatibility,
        )
        .unwrap();
        let area_row = sounds.row_by("Sound", "audio_telemetry_a1").unwrap();
        assert_eq!(sounds.get(&area_row, "IsAmbientScene"), Some("1"));
        assert_eq!(sounds.get(&area_row, "Loop"), Some("1"));
        assert!(definitions
            .iter()
            .any(|definition| definition.marker == TelemetryMarker::Frontend));
        assert_eq!(
            sounds.row_by("Sound", "music_options").unwrap()[sounds.column("FileName").unwrap()],
            "audio_telemetry\\frontend\\music_options.flac"
        );
        assert_eq!(
            sounds.row_by("Sound", "act1_scene_front_end").unwrap()
                [sounds.column("FileName").unwrap()],
            "audio_telemetry\\frontend\\act1_scene_front_end.flac"
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.marker == TelemetryMarker::Frontend)
                .count(),
            4
        );
    }

    #[test]
    fn generated_marker_flac_self_verifies() {
        let root = std::env::temp_dir().join(format!("d2rhub-audio-v5-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("a137.flac");
        let confidence = write_marker_flac(
            &path,
            TelemetryMarker::Area { area_id: 137 },
            None,
            MarkerConfig::default(),
        )
        .unwrap();
        assert!(path.is_file());
        assert!(confidence > 0.7);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terror_entry_sound_is_tagged_at_the_immediate_redirect_target() {
        let sounds = TsvTable::parse(
            "sounds",
            "Sound\t*Index\tRedirect\tFileName\tChannel\tIsAmbientScene\tIsAmbientEvent\tVolume Min\tVolume Max\tPitch Min\tPitch Max\tGroup Size\tGroup Weight\tLoop\tDefer Inst\tStop Inst\tCompound\tStream\tTracking\tIs2D\tHDOptOut\nitem_rune_hd\t1\t\titem\\rune.flac\tsfx/items_hd\t0\t0\t200\t200\t100\t100\t0\t0\t0\t0\t1\t0\t0\t0\t0\t0\nscene_wilderness_day\t2\t\tambient\\scene.flac\tsfx/ambient/scene-2d_hd\t1\t0\t200\t200\t100\t100\t0\t0\t1\t0\t0\t0\t1\t0\t1\t0\nmusic_options\t3\t\tcommon\\options.flac\tmusic_sd\t0\t0\t127\t127\t100\t100\t0\t0\t1\t1\t0\t0\t1\t0\t1\t0\ndesecrated_enter\t4\tdesecrated_enter_hd\tquest\\desecrated_enter.flac\tsfx/ambient/event-3d_sd\t0\t0\t255\t255\t100\t100\t0\t0\t0\t1\t0\t0\t1\t0\t0\t0\ndesecrated_enter_hd\t5\t\tquest\\desecrated_enter_hd.flac\tsfx/ambient/event-3d_hd\t0\t0\t255\t255\t100\t100\t0\t0\t0\t1\t0\t0\t1\t0\t0\t0\n",
        )
        .unwrap();
        let mut compatibility = Vec::new();
        let (sounds, definitions) =
            patch_sounds(sounds, &[], &[], &[], &HashMap::new(), &mut compatibility).unwrap();

        let sd = sounds.row_by("Sound", "desecrated_enter").unwrap();
        assert_eq!(sounds.get(&sd, "Redirect"), Some("desecrated_enter_hd"));
        let hd = sounds.row_by("Sound", "desecrated_enter_hd").unwrap();
        assert_eq!(
            sounds.get(&hd, "FileName"),
            Some("audio_telemetry\\terror\\desecrated_enter_hd.flac")
        );
        let immediate = definitions
            .iter()
            .filter(|definition| {
                definition.marker
                    == (TelemetryMarker::Area {
                        area_id: TERROR_PROBE_MARKER_AREA_ID,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(immediate.len(), 1);
        assert_eq!(
            immediate[0].source_filename.as_deref(),
            Some("quest\\desecrated_enter_hd.flac")
        );
        assert!(compatibility
            .iter()
            .any(|item| { item.action == "mix_immediate_terror_presence_marker" }));
    }

    #[test]
    fn missing_terror_entry_sound_fails_instead_of_silently_using_delayed_probe() {
        let sounds = TsvTable::parse(
            "sounds",
            "Sound\t*Index\tRedirect\tFileName\tIsAmbientScene\nitem_rune_hd\t1\t\titem\\rune.flac\t0\nscene_wilderness_day\t2\t\tambient\\scene.flac\t1\nmusic_options\t3\t\tcommon\\options.flac\t0\n",
        )
        .unwrap();
        let error =
            patch_sounds(sounds, &[], &[], &[], &HashMap::new(), &mut Vec::new()).unwrap_err();
        assert!(error.contains("缺少恐怖区域即时入场声音"));
    }

    #[test]
    fn invalid_terror_entry_redirects_fail_with_a_specific_error() {
        let missing_target = TsvTable::parse(
            "sounds",
            "Sound\t*Index\tRedirect\tFileName\tIsAmbientScene\nitem_rune_hd\t1\t\titem\\rune.flac\t0\nscene_wilderness_day\t2\t\tambient\\scene.flac\t1\nmusic_options\t3\t\tcommon\\options.flac\t0\ndesecrated_enter\t4\tmissing_enter_sound\tquest\\desecrated_enter.flac\t0\n",
        )
        .unwrap();
        let missing_error = patch_sounds(
            missing_target,
            &[],
            &[],
            &[],
            &HashMap::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(missing_error.contains("重定向到不存在的声音 missing_enter_sound"));

        let cycle = TsvTable::parse(
            "sounds",
            "Sound\t*Index\tRedirect\tFileName\tIsAmbientScene\nitem_rune_hd\t1\t\titem\\rune.flac\t0\nscene_wilderness_day\t2\t\tambient\\scene.flac\t1\nmusic_options\t3\t\tcommon\\options.flac\t0\ndesecrated_enter\t4\tdesecrated_enter_hd\tquest\\desecrated_enter.flac\t0\ndesecrated_enter_hd\t5\tdesecrated_enter\tquest\\desecrated_enter_hd.flac\t0\n",
        )
        .unwrap();
        let cycle_error =
            patch_sounds(cycle, &[], &[], &[], &HashMap::new(), &mut Vec::new()).unwrap_err();
        assert!(cycle_error.contains("Redirect 形成循环"));
    }

    #[test]
    fn cloned_item_assets_include_inventory_sprite_variants() {
        let root = std::env::temp_dir().join(format!(
            "d2rhub-audio-item-sprites-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("data/hd/global/ui/items/misc/jewel/jewel1.sprite");
        let lowend = root.join("data/hd/global/ui/items/misc/jewel/jewel1.lowend.sprite");
        write_file(&source, b"hd-sprite").unwrap();
        write_file(&lowend, b"lowend-sprite").unwrap();

        let copied = copy_item_ui_sprites(
            &root,
            None,
            &["jewel/jewel"],
            "audio_telemetry/items/i39_jew",
        )
        .unwrap();
        assert_eq!(copied, 2);
        assert_eq!(
            std::fs::read(
                root.join("data/hd/global/ui/items/misc/audio_telemetry/items/i39_jew1.sprite")
            )
            .unwrap(),
            b"hd-sprite"
        );
        assert_eq!(
            std::fs::read(
                root.join(
                    "data/hd/global/ui/items/misc/audio_telemetry/items/i39_jew1.lowend.sprite"
                )
            )
            .unwrap(),
            b"lowend-sprite"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn item_entity_resolution_falls_back_by_item_code_mapping() {
        let root = std::env::temp_dir().join(format!(
            "d2rhub-audio-item-entity-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let baseline = root.join("data/hd/items/misc/key/base_key.json");
        write_file(&baseline, br#"{"entities":[]}"#).unwrap();

        let resolved =
            resolve_item_entity_asset(&root, None, "key/custom_icon_only", Some("key/base_key"))
                .unwrap();

        assert_eq!(resolved.asset, "key/base_key");
        assert!(resolved.used_baseline_mapping);
        assert!(resolved.preferred_error.is_some());
        assert_eq!(resolved.document["entities"], serde_json::json!([]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn item_sprite_resolution_uses_each_candidates_available_variant() {
        let root = std::env::temp_dir().join(format!(
            "d2rhub-audio-item-sprite-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let preferred = root.join("data/hd/global/ui/items/misc/key/custom_icon_only.sprite");
        let baseline = root.join("data/hd/global/ui/items/misc/key/base_key.lowend.sprite");
        write_file(&preferred, b"custom-hd-sprite").unwrap();
        write_file(&baseline, b"baseline-lowend-sprite").unwrap();

        let copied = copy_item_ui_sprites(
            &root,
            None,
            &["key/custom_icon_only", "key/base_key"],
            "audio_telemetry/items/i43_key",
        )
        .unwrap();

        assert_eq!(copied, 2);
        let target = root.join("data/hd/global/ui/items/misc/audio_telemetry/items");
        assert_eq!(
            std::fs::read(target.join("i43_key.sprite")).unwrap(),
            b"custom-hd-sprite"
        );
        assert_eq!(
            std::fs::read(target.join("i43_key.lowend.sprite")).unwrap(),
            b"baseline-lowend-sprite"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_build_removes_only_its_transaction_directory() {
        let root = std::env::temp_dir().join(format!("d2rhub-audio-fail-{}", uuid::Uuid::new_v4()));
        let source = root.join("broken.mpq");
        let excel = source.join("data/global/excel");
        let output = root.join("mods");
        std::fs::create_dir_all(&excel).unwrap();
        std::fs::write(
            excel.join("misc.txt"),
            "name\tcode\tdropsound\nEl Rune\tr01\titem_rune_hd\n",
        )
        .unwrap();
        std::fs::write(
            excel.join("sounds.txt"),
            "Sound\t*Index\tRedirect\tFileName\tIsAmbientScene\nitem_rune_hd\t1\t\titem\\rune.flac\t0\nscene_wilderness_day\t2\t\tambient\\scene.flac\t1\n",
        )
        .unwrap();
        std::fs::write(
            excel.join("levels.txt"),
            "Name\tId\tSoundEnv\tLevelName\nAct 1 - Town\t1\t1\tRogue Encampment\nAct 1 - Wilderness 5\t6\t2\tBlack Marsh\nAct 1 - Crypt 1\t20\t2\tForgotten Tower\nAct 1 - Crypt 2\t21\t2\tTower Cellar Level 1\nAct 1 - Crypt 3\t22\t2\tTower Cellar Level 2\nAct 1 - Crypt 4\t23\t2\tTower Cellar Level 3\nAct 1 - Crypt 5\t24\t2\tTower Cellar Level 4\nAct 1 - Crypt 6\t25\t2\tTower Cellar Level 5\n",
        )
        .unwrap();
        std::fs::write(
            excel.join("soundenviron.txt"),
            "Handle\tIndex\tDay Ambience\tHD Day Ambience\tNight Ambience\tHD Night Ambience\nTown\t1\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\nWild\t2\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\n",
        )
        .unwrap();
        let error = build(BuildAudioModRequest {
            build_mode: AudioModBuildMode::Augment,
            area_coverage: AudioAreaCoverage::CountessRoute,
            tracked_categories: vec![CATEGORY_RUNES.to_string()],
            source_directory: Some(source.to_string_lossy().to_string()),
            game_directory: None,
            output_directory: Some(output.to_string_lossy().to_string()),
            mod_name: None,
            sound_environment_file: None,
            gain_db: None,
        })
        .unwrap_err();
        assert!(error.contains("r02"));
        assert!(!output.join("broken-AudioTelemetry").exists());
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_a_complete_mod_with_silent_rune_heartbeats() {
        let root = std::env::temp_dir().join(format!("d2rhub-audio-mod-{}", uuid::Uuid::new_v4()));
        let source = root.join("jcy.mpq");
        let excel = source.join("data/global/excel");
        let output = root.join("mods");
        std::fs::create_dir_all(&excel).unwrap();

        let mut misc = "name\tcode\tdropsound\tusesound\n".to_string();
        for number in 1..=33 {
            misc.push_str(&format!(
                "Rune {number}\tr{number:02}\titem_rune_hd\titem_rune_hd\n"
            ));
        }
        std::fs::write(excel.join("misc.txt"), misc).unwrap();
        std::fs::write(
            excel.join("sounds.txt"),
            "Sound\t*Index\tRedirect\tFileName\tChannel\tIsAmbientScene\tIsAmbientEvent\tVolume Min\tVolume Max\tPitch Min\tPitch Max\tGroup Size\tGroup Weight\tLoop\tDefer Inst\tStop Inst\tCompound\tStream\tTracking\tIs2D\tHDOptOut\nitem_rune_hd\t10\t\titem\\rune.flac\tsfx/items_hd\t0\t0\t200\t200\t100\t100\t0\t0\t0\t0\t1\t0\t0\t0\t0\t0\nscene_wilderness_day\t11\t\tambient\\scene.flac\tsfx/ambient/scene-2d_hd\t1\t0\t200\t200\t100\t100\t0\t0\t1\t0\t0\t0\t1\t0\t1\t0\nmusic_options\t12\t\tcommon\\options.flac\tmusic_sd\t0\t0\t127\t127\t100\t100\t0\t0\t1\t1\t0\t0\t1\t0\t1\t0\ndesecrated_enter\t13\tdesecrated_enter_hd\tquest\\desecrated_enter.flac\tsfx/ambient/event-3d_sd\t0\t0\t255\t255\t100\t100\t0\t0\t0\t1\t0\t0\t1\t0\t0\t0\ndesecrated_enter_hd\t14\t\tquest\\desecrated_enter_hd.flac\tsfx/ambient/event-3d_hd\t0\t0\t255\t255\t100\t100\t0\t0\t0\t1\t0\t0\t1\t0\t0\t0\n",
        )
        .unwrap();
        std::fs::write(
            excel.join("levels.txt"),
            "Name\tId\tSoundEnv\tLevelName\nNull\t0\t0\tNull\nAct 1 - Town\t1\t1\tRogue Encampment\nAct 1 - Wilderness 5\t6\t2\tBlack Marsh\nAct 1 - Crypt 1\t20\t2\tForgotten Tower\nAct 1 - Crypt 2\t21\t2\tTower Cellar Level 1\nAct 1 - Crypt 3\t22\t2\tTower Cellar Level 2\nAct 1 - Crypt 4\t23\t2\tTower Cellar Level 3\nAct 1 - Crypt 5\t24\t2\tTower Cellar Level 4\nAct 1 - Crypt 6\t25\t2\tTower Cellar Level 5\n",
        )
        .unwrap();
        std::fs::write(
            excel.join("soundenviron.txt"),
            "Handle\tIndex\tDay Ambience\tHD Day Ambience\tNight Ambience\tHD Night Ambience\nTown\t1\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\nWild\t2\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\tscene_wilderness_day\n",
        )
        .unwrap();
        write_rune_unit_definitions(&source);
        std::fs::write(
            source.join("data/hd/items/misc/rune/el_rune.json"),
            r#"{
                // Some existing Mods use the JSON5 syntax accepted by the game tools.
                dependencies: {
                    json: [{ path: 'data/hd/items/dropped_items/dropped_items_helms_flip_ne.json' }],
                },
                type: 'UnitDefinition',
                name: 'el_rune',
                entities: [{
                    type: 'Entity',
                    name: 'entity_root',
                    id: 1000,
                    components: [{
                        type: 'UnitRootComponent',
                        name: 'component_root',
                        state_machine_filename: 'data/hd/items/dropped_items/dropped_items_helms_flip_ne.json',
                    }],
                }],
            }"#,
        )
        .unwrap();
        let samples = (0..24_000)
            .map(|index| {
                ((std::f32::consts::TAU * 880.0 * index as f32 / 48_000.0).sin() * 4_000.0) as i32
            })
            .collect::<Vec<_>>();
        for source_audio in [
            source.join("data/hd/global/sfx/ambient/scene.flac"),
            source.join("data/hd/global/sfx/quest/desecrated_enter_hd.flac"),
            source.join("data/hd/global/music/common/options_hd.flac"),
        ] {
            std::fs::create_dir_all(source_audio.parent().unwrap()).unwrap();
            encode_flac(&source_audio, &samples, 48_000, 1, 16).unwrap();
        }
        std::fs::write(
            source.join("modinfo.json"),
            "{ name: 'Original Mod', author: 'Preserved Author', savepath: '../', }",
        )
        .unwrap();

        let report = build(BuildAudioModRequest {
            build_mode: AudioModBuildMode::Augment,
            area_coverage: AudioAreaCoverage::CountessRoute,
            tracked_categories: vec![CATEGORY_RUNES.to_string()],
            source_directory: Some(source.to_string_lossy().to_string()),
            game_directory: None,
            output_directory: Some(output.to_string_lossy().to_string()),
            mod_name: Some("Countess-Audio-Test".to_string()),
            sound_environment_file: None,
            gain_db: Some(-26.0),
        })
        .unwrap();
        assert_eq!(report.rune_assets.len(), 33);
        assert_eq!(report.area_assets.len(), COUNTESS_AREA_IDS.len());
        assert_eq!(report.terror_assets.len(), 2);
        assert!(report
            .terror_assets
            .iter()
            .all(|asset| asset.confidence > 0.7));
        assert_eq!(report.frontend_assets.len(), 1);
        assert_eq!(
            report
                .rune_assets
                .iter()
                .filter(|asset| asset.preserved_source_audio)
                .count(),
            0
        );
        assert_eq!(report.mod_name, "Countess-Audio-Test");
        assert_eq!(report.launch_arguments, "-mod Countess-Audio-Test -txt");
        assert_eq!(report.recipe_version, AUDIO_MOD_RECIPE_VERSION);
        let output_modinfo = serde_json::from_str::<serde_json::Value>(
            &read_utf8(&output.join("Countess-Audio-Test/Countess-Audio-Test.mpq/modinfo.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(output_modinfo["author"], "Preserved Author");
        assert_eq!(output_modinfo["name"], "Countess-Audio-Test");
        assert!(report
            .capabilities
            .iter()
            .any(|value| value == TERROR_IMMEDIATE_ENTRY_CAPABILITY));
        assert_eq!(report.source_mod_name.as_deref(), Some("jcy"));
        let output_mpq = output
            .join("Countess-Audio-Test")
            .join("Countess-Audio-Test.mpq");
        assert!(output_mpq
            .join("data/global/excel/soundenviron.txt")
            .is_file());
        assert!(output_mpq
            .join("data/hd/global/sfx/audio_telemetry/runes/r01.flac")
            .is_file());
        assert!(output_mpq
            .join("data/hd/global/sfx/audio_telemetry/areas/a6.flac")
            .is_file());
        let misc_output = TsvTable::parse(
            "misc",
            &read_utf8(&output_mpq.join("data/global/excel/misc.txt")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            misc_output.get(&misc_output.rows[0], "dropsound"),
            Some("item_rune_hd")
        );
        assert_eq!(
            misc_output.get(&misc_output.rows[0], "usesound"),
            Some("item_rune_hd")
        );
        let el_document: serde_json::Value = serde_json::from_str(
            &read_utf8(&output_mpq.join("data/hd/items/misc/rune/el_rune.json")).unwrap(),
        )
        .unwrap();
        let unit_root = el_document["entities"][0]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|component| component["type"] == "UnitRootComponent")
            .unwrap();
        assert_eq!(
            unit_root["state_machine_filename"],
            "data/hd/items/audio_telemetry/runes/r01_ground_heartbeat.json"
        );
        let heartbeat: serde_json::Value = serde_json::from_str(
            &read_utf8(
                &output_mpq.join("data/hd/items/audio_telemetry/runes/r01_ground_heartbeat.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(heartbeat["states"][0]["audioId"], "audio_telemetry_r01");
        assert_eq!(heartbeat["states"][1]["audioId"], "");
        assert_eq!(heartbeat["transitions"].as_array().unwrap().len(), 2);
        assert_eq!(heartbeat["states"][0]["customPreservedField"], 42);
        assert_eq!(
            heartbeat["dependencies"]["particles"][0]["path"],
            "data/hd/vfx2/custom-preserved.particles"
        );
        assert!(output_mpq
            .join("data/global/music/audio_telemetry/frontend/music_options.flac")
            .is_file());
        assert!(Path::new(&report.mod_directory)
            .join(AREA_CATALOG_FILE_NAME)
            .is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires D2RHUB_AUDIO_REAL_MOD_SOURCE"]
    fn builds_real_source_mod_from_environment() {
        let source = std::env::var("D2RHUB_AUDIO_REAL_MOD_SOURCE").unwrap();
        let temporary_root =
            std::env::temp_dir().join(format!("d2rhub-audio-real-{}", uuid::Uuid::new_v4()));
        let output = std::env::var("D2RHUB_AUDIO_REAL_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| temporary_root.join("mods"));
        let report = build(BuildAudioModRequest {
            build_mode: AudioModBuildMode::Augment,
            area_coverage: AudioAreaCoverage::CountessRoute,
            tracked_categories: default_tracked_categories(),
            source_directory: Some(source),
            game_directory: std::env::var("D2RHUB_AUDIO_GAME_ROOT").ok(),
            output_directory: Some(output.to_string_lossy().to_string()),
            mod_name: None,
            sound_environment_file: std::env::var("D2RHUB_AUDIO_REAL_SOUND_ENVIRON").ok(),
            gain_db: Some(-30.0),
        })
        .unwrap();
        assert_eq!(report.rune_assets.len(), RUNE_COUNT as usize);
        assert_eq!(report.item_assets.len(), 50);
        assert_eq!(report.area_assets.len(), COUNTESS_AREA_IDS.len());
        assert!(report
            .rune_assets
            .iter()
            .all(|asset| asset.confidence > 0.7));
        assert!(report
            .area_assets
            .iter()
            .all(|asset| asset.confidence > 0.7));
        if std::env::var_os("D2RHUB_AUDIO_REAL_OUTPUT").is_some() {
            assert!(report
                .area_assets
                .iter()
                .all(|asset| asset.preserved_source_audio));
        }
        if std::env::var_os("D2RHUB_AUDIO_REAL_OUTPUT").is_none() {
            std::fs::remove_dir_all(temporary_root).unwrap();
        }
    }

    #[test]
    #[ignore = "requires D2RHUB_AUDIO_GAME_ROOT"]
    fn builds_minimal_mod_from_game_storage() {
        let game_root = std::env::var("D2RHUB_AUDIO_GAME_ROOT").unwrap();
        let temporary_root =
            std::env::temp_dir().join(format!("d2rhub-audio-minimal-{}", uuid::Uuid::new_v4()));
        let output = temporary_root.join("mods");
        let report = build(BuildAudioModRequest {
            build_mode: AudioModBuildMode::Minimal,
            area_coverage: AudioAreaCoverage::CountessRoute,
            tracked_categories: default_tracked_categories(),
            source_directory: None,
            game_directory: Some(game_root),
            output_directory: Some(output.to_string_lossy().to_string()),
            mod_name: None,
            sound_environment_file: None,
            gain_db: Some(-30.0),
        })
        .unwrap();
        assert_eq!(report.build_mode, AudioModBuildMode::Minimal);
        assert!(!report.source_mod_copied);
        assert_eq!(report.rune_assets.len(), RUNE_COUNT as usize);
        assert_eq!(report.item_assets.len(), 50);
        assert_eq!(report.area_assets.len(), COUNTESS_AREA_IDS.len());
        assert!(!report.frontend_assets.is_empty());
        assert!(report
            .frontend_assets
            .iter()
            .all(|asset| asset.preserved_source_audio && asset.confidence > 0.7));
        std::fs::remove_dir_all(temporary_root).unwrap();
    }

    #[test]
    #[ignore = "requires D2RHUB_AUDIO_GAME_ROOT"]
    fn validates_every_game_area_and_ambience_from_storage() {
        let game_root = std::env::var("D2RHUB_AUDIO_GAME_ROOT").unwrap();
        let storage = casc_core::Storage::open(&game_root).unwrap();
        let levels = TsvTable::parse(
            "levels.txt",
            &read_casc_utf8(&storage, "data/global/excel/levels.txt").unwrap(),
        )
        .unwrap();
        let sounds = TsvTable::parse(
            "sounds.txt",
            &read_casc_utf8(&storage, "data/global/excel/sounds.txt").unwrap(),
        )
        .unwrap();
        let environments = TsvTable::parse(
            "soundenviron.txt",
            &read_casc_utf8(&storage, "data/global/excel/soundenviron.txt").unwrap(),
        )
        .unwrap();
        let missing_mod =
            std::env::temp_dir().join(format!("d2rhub-audio-no-mod-{}", uuid::Uuid::new_v4()));
        let localization = load_area_localization(&missing_mod, Some(&storage)).unwrap();
        let areas = collect_areas_localized(&levels, &localization).unwrap();
        assert_eq!(areas.len(), 137);
        assert_eq!(
            areas
                .iter()
                .find(|area| area.area_id == 104)
                .unwrap()
                .scene_name,
            "外域荒原"
        );
        let filenames =
            collect_area_ambience_filenames(&levels, &environments, &sounds, &areas).unwrap();
        assert_eq!(filenames.len(), areas.len());
        for filename in filenames.values() {
            let path = format!(
                "data:data\\hd\\global\\sfx\\{}",
                filename.replace('/', "\\")
            );
            storage
                .read_n(&path, 32)
                .unwrap_or_else(|error| panic!("missing {path}: {error}"));
        }
    }
}
