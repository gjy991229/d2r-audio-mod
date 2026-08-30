use crate::generator::{self, AudioAreaCoverage, AudioModBuildMode, BuildAudioModRequest};
use d2r_audio_protocol::item_catalog::default_tracked_categories;
use std::ffi::{c_void, OsStr};
use std::iter;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetStockObject, UpdateWindow, COLOR_WINDOW, DEFAULT_GUI_FONT,
};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Console::FreeConsole;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetPathFromIDListW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE,
    BIF_RETURNONLYFSDIRS, BROWSEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, TranslateMessage, BS_DEFPUSHBUTTON, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, IDC_ARROW, MSG,
    SW_SHOW, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_MINIMIZEBOX, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

const WINDOW_CLASS: &str = "D2RAudioModGeneratorWindow";
const WINDOW_TITLE: &str = concat!("D2R 声纹 MOD 生成器 v", env!("CARGO_PKG_VERSION"));
const DEFAULT_MOD_NAME: &str = "D2RAudioTelemetry";

const ID_SOURCE_EDIT: usize = 101;
const ID_SOURCE_BROWSE: usize = 102;
const ID_NAME_EDIT: usize = 201;
const ID_OUTPUT_EDIT: usize = 301;
const ID_OUTPUT_BROWSE: usize = 302;
const ID_GENERATE: usize = 401;
const ID_OPEN_RESULT: usize = 402;
const ID_STATUS: usize = 501;
const WM_BUILD_FINISHED: u32 = WM_APP + 17;

type BuildResult = Result<generator::BuildAudioModReport, String>;

struct AppState {
    source_edit: HWND,
    source_browse: HWND,
    name_edit: HWND,
    output_edit: HWND,
    output_browse: HWND,
    generate_button: HWND,
    open_button: HWND,
    status: HWND,
    last_output: Option<PathBuf>,
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(iter::once(0)).collect()
}

unsafe fn control_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let written = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
    String::from_utf16_lossy(&buffer[..written.max(0) as usize])
}

unsafe fn set_text(control: HWND, value: &str) {
    let value = wide(value);
    SetWindowTextW(control, value.as_ptr());
}

unsafe fn set_font(control: HWND) {
    let font = GetStockObject(DEFAULT_GUI_FONT);
    SendMessageW(control, WM_SETFONT, font as usize, 1);
}

// Keeping the layout values beside each call makes this small fixed Win32 form easier to audit.
#[allow(clippy::too_many_arguments)]
unsafe fn create_control(
    class_name: &str,
    text: &str,
    style: u32,
    extended_style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: usize,
    instance: HINSTANCE,
) -> Result<HWND, String> {
    let class_name = wide(class_name);
    let text = wide(text);
    let control = CreateWindowExW(
        extended_style,
        class_name.as_ptr(),
        text.as_ptr(),
        style,
        x,
        y,
        width,
        height,
        parent,
        id as *mut c_void,
        instance,
        null(),
    );
    if control.is_null() {
        return Err(format!("创建窗口控件失败（编号 {id}）"));
    }
    set_font(control);
    Ok(control)
}

fn is_game_root(path: &Path) -> bool {
    path.join(".build.info").is_file() && path.join("Data").is_dir()
}

fn game_root_from(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|candidate| is_game_root(candidate))
        .map(Path::to_path_buf)
}

fn discover_game_root(source: Option<&Path>, output: Option<&Path>) -> Option<PathBuf> {
    for path in output.into_iter().chain(source) {
        if let Some(root) = game_root_from(path) {
            return Some(root);
        }
    }

    if let Some(value) = std::env::var_os("D2R_GAME_DIR") {
        let candidate = PathBuf::from(value);
        if is_game_root(&candidate) {
            return Some(candidate);
        }
    }

    for path in std::env::current_exe()
        .ok()
        .into_iter()
        .chain(std::env::current_dir().ok())
    {
        if let Some(root) = game_root_from(&path) {
            return Some(root);
        }
    }

    let mut candidates = Vec::new();
    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(directory) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(directory).join("Diablo II Resurrected"));
        }
    }
    let fixed_drives = unsafe { GetLogicalDrives() };
    for drive in b'A'..=b'Z' {
        let bit = 1u32 << (drive - b'A');
        if fixed_drives & bit == 0 {
            continue;
        }
        let root = PathBuf::from(format!("{}:\\", drive as char));
        let root_wide = wide(root.as_os_str());
        if unsafe { GetDriveTypeW(root_wide.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        for suffix in [
            "Diablo II Resurrected",
            "Games\\Diablo II Resurrected",
            "Blizzard\\Diablo II Resurrected",
            "Battle.net\\Diablo II Resurrected",
        ] {
            candidates.push(root.join(suffix));
        }
    }
    candidates
        .into_iter()
        .find(|candidate| is_game_root(candidate))
}

fn default_output_directory() -> String {
    discover_game_root(None, None)
        .map(|root| root.join("mods").to_string_lossy().into_owned())
        .unwrap_or_default()
}

unsafe fn browse_for_folder(owner: HWND, title: &str) -> Option<PathBuf> {
    let title = wide(title);
    let mut display_name = [0u16; 260];
    let info = BROWSEINFOW {
        hwndOwner: owner,
        pidlRoot: null_mut(),
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let item = SHBrowseForFolderW(&info);
    if item.is_null() {
        return None;
    }
    let mut path = [0u16; 260];
    let ok = SHGetPathFromIDListW(item, path.as_mut_ptr());
    CoTaskMemFree(item.cast());
    if ok == 0 {
        return None;
    }
    let length = path
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(path.len());
    Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
}

unsafe fn state_from_window(window: HWND) -> Option<&'static mut AppState> {
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut AppState;
    pointer.as_mut()
}

unsafe fn set_busy(state: &AppState, busy: bool) {
    let enabled = if busy { 0 } else { 1 };
    for control in [
        state.source_edit,
        state.source_browse,
        state.name_edit,
        state.output_edit,
        state.output_browse,
        state.generate_button,
    ] {
        EnableWindow(control, enabled);
    }
    if busy {
        EnableWindow(state.open_button, 0);
    }
}

unsafe fn start_build(window: HWND, state: &mut AppState) {
    let source = control_text(state.source_edit).trim().to_string();
    let mod_name = control_text(state.name_edit).trim().to_string();
    let output = control_text(state.output_edit).trim().to_string();

    if mod_name.is_empty() {
        set_text(state.status, "请填写生成的 MOD 名称。");
        return;
    }
    if output.is_empty() {
        set_text(
            state.status,
            "请选择输出目录。建议直接选择《暗黑破坏神 II：狱火重生》安装目录下的 mods 文件夹。",
        );
        return;
    }

    let source_path = (!source.is_empty()).then(|| PathBuf::from(&source));
    let output_path = PathBuf::from(&output);
    let game_root = discover_game_root(source_path.as_deref(), Some(&output_path));
    let mode = if source_path.is_some() {
        AudioModBuildMode::Augment
    } else {
        AudioModBuildMode::Minimal
    };
    let request = BuildAudioModRequest {
        build_mode: mode,
        source_directory: source_path.map(|path| path.to_string_lossy().into_owned()),
        game_directory: game_root.map(|path| path.to_string_lossy().into_owned()),
        area_coverage: AudioAreaCoverage::AllAreas,
        tracked_categories: default_tracked_categories(),
        output_directory: Some(output),
        mod_name: Some(mod_name),
        sound_environment_file: None,
        gain_db: None,
    };

    state.last_output = None;
    set_busy(state, true);
    set_text(
        state.status,
        "正在生成全量声纹 MOD……首次需要从游戏资源中提取文件，可能需要一点时间，请勿关闭窗口。",
    );

    let window_value = window as isize;
    std::thread::spawn(move || {
        let result: BuildResult = generator::build(request);
        let pointer = Box::into_raw(Box::new(result));
        let posted = unsafe {
            PostMessageW(
                window_value as HWND,
                WM_BUILD_FINISHED,
                0,
                pointer as LPARAM,
            )
        };
        if posted == 0 {
            unsafe {
                drop(Box::from_raw(pointer));
            }
        }
    });
}

unsafe fn finish_build(state: &mut AppState, result: BuildResult) {
    set_busy(state, false);
    match result {
        Ok(report) => {
            state.last_output = Some(PathBuf::from(&report.mod_directory));
            EnableWindow(state.open_button, 1);
            set_text(
                state.status,
                &format!(
                    "生成成功！\r\n位置：{}\r\n游戏启动参数：{}\r\n\r\n已包含：全区域、全部支持物品、主界面识别。源 MOD 没有被修改。",
                    report.mod_directory, report.launch_arguments
                ),
            );
        }
        Err(error) => {
            let hint = if error.contains("游戏目录") || error.contains("CASC") {
                "\r\n\r\n请把输出目录选为 D2R 安装目录中的 mods 文件夹，工具会由此自动找到游戏资源。"
            } else {
                ""
            };
            set_text(state.status, &format!("生成失败：{error}{hint}"));
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = wparam & 0xffff;
            let Some(state) = state_from_window(window) else {
                return DefWindowProcW(window, message, wparam, lparam);
            };
            match id {
                ID_SOURCE_BROWSE => {
                    if let Some(path) = browse_for_folder(
                        window,
                        "选择源 MOD 文件夹（可选择外层 MOD 文件夹或 .mpq 文件夹）",
                    ) {
                        set_text(state.source_edit, &path.to_string_lossy());
                    }
                    0
                }
                ID_OUTPUT_BROWSE => {
                    if let Some(mut path) = browse_for_folder(
                        window,
                        "选择输出父目录（推荐选择 D2R 安装目录中的 mods 文件夹）",
                    ) {
                        if is_game_root(&path) {
                            path.push("mods");
                        }
                        set_text(state.output_edit, &path.to_string_lossy());
                    }
                    0
                }
                ID_GENERATE => {
                    start_build(window, state);
                    0
                }
                ID_OPEN_RESULT => {
                    if let Some(path) = state.last_output.as_ref() {
                        let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
                    }
                    0
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
        WM_BUILD_FINISHED => {
            let pointer = lparam as *mut BuildResult;
            if !pointer.is_null() {
                let result = *Box::from_raw(pointer);
                if let Some(state) = state_from_window(window) {
                    finish_build(state, result);
                }
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            let pointer = SetWindowLongPtrW(window, GWLP_USERDATA, 0) as *mut AppState;
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_app_window() -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    if instance.is_null() {
        return Err("无法取得程序实例".to_string());
    }
    let class_name = wide(WINDOW_CLASS);
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW as usize + 1) as *mut c_void,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
    };
    if RegisterClassW(&class) == 0 {
        return Err("注册生成器窗口失败".to_string());
    }

    let title = wide(WINDOW_TITLE);
    let window = CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        680,
        430,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if window.is_null() {
        return Err("创建生成器窗口失败".to_string());
    }

    let static_style = WS_CHILD | WS_VISIBLE;
    create_control(
        "STATIC",
        "只需选择是否加工现有 MOD。地图与物品固定全量，使用稳定默认声纹参数。",
        static_style,
        0,
        24,
        20,
        620,
        36,
        window,
        0,
        instance,
    )?;
    create_control(
        "STATIC",
        "源 MOD（可不选）",
        static_style,
        0,
        24,
        70,
        180,
        20,
        window,
        0,
        instance,
    )?;
    let source_edit = create_control(
        "EDIT",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        24,
        94,
        526,
        27,
        window,
        ID_SOURCE_EDIT,
        instance,
    )?;
    let source_browse = create_control(
        "BUTTON",
        "选择…",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        560,
        93,
        86,
        29,
        window,
        ID_SOURCE_BROWSE,
        instance,
    )?;
    create_control(
        "STATIC",
        "生成的 MOD 名称",
        static_style,
        0,
        24,
        138,
        180,
        20,
        window,
        0,
        instance,
    )?;
    let name_edit = create_control(
        "EDIT",
        DEFAULT_MOD_NAME,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        24,
        162,
        622,
        27,
        window,
        ID_NAME_EDIT,
        instance,
    )?;
    create_control(
        "STATIC",
        "输出到（父目录）",
        static_style,
        0,
        24,
        206,
        180,
        20,
        window,
        0,
        instance,
    )?;
    let output_edit = create_control(
        "EDIT",
        &default_output_directory(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        24,
        230,
        526,
        27,
        window,
        ID_OUTPUT_EDIT,
        instance,
    )?;
    let output_browse = create_control(
        "BUTTON",
        "选择…",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        560,
        229,
        86,
        29,
        window,
        ID_OUTPUT_BROWSE,
        instance,
    )?;
    let generate_button = create_control(
        "BUTTON",
        "生成 MOD",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        24,
        278,
        128,
        34,
        window,
        ID_GENERATE,
        instance,
    )?;
    let open_button = create_control(
        "BUTTON",
        "打开生成目录",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        162,
        278,
        128,
        34,
        window,
        ID_OPEN_RESULT,
        instance,
    )?;
    EnableWindow(open_button, 0);
    let status = create_control(
        "EDIT",
        "不选择源 MOD：创建独立最小 MOD。选择源 MOD：保留原内容并附加声纹。",
        WS_CHILD
            | WS_VISIBLE
            | WS_BORDER
            | WS_VSCROLL
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | ES_READONLY as u32,
        WS_EX_CLIENTEDGE,
        308,
        276,
        338,
        82,
        window,
        ID_STATUS,
        instance,
    )?;

    let state = Box::new(AppState {
        source_edit,
        source_browse,
        name_edit,
        output_edit,
        output_browse,
        generate_button,
        open_button,
        status,
        last_output: None,
    });
    SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
    Ok(window)
}

pub fn run(detach_console: bool) -> Result<(), String> {
    unsafe {
        let com_result = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let com_initialized = com_result >= 0;
        let window = match create_app_window() {
            Ok(window) => window,
            Err(error) => {
                if com_initialized {
                    CoUninitialize();
                }
                return Err(error);
            }
        };
        if detach_console {
            FreeConsole();
        }
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);

        let mut message: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                if com_initialized {
                    CoUninitialize();
                }
                return Err("读取窗口消息失败".to_string());
            }
            if result == 0 {
                break;
            }
            if IsDialogMessageW(window, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if com_initialized {
            CoUninitialize();
        }
    }
    Ok(())
}
