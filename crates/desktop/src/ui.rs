#![allow(unsafe_op_in_unsafe_fn)]

use crate::input::{InputMode, InputState};
use ptools_core::*;
use ptools_ui as theme;
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    sync::mpsc::{self, Receiver, Sender},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        LibraryLoader::GetModuleHandleW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Registry::*,
        Threading::CreateMutexW,
    },
    UI::{
        Controls::Dialogs::*, Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
        WindowsAndMessaging::*,
    },
};

const WM_SHOW: u32 = WM_APP + 1;
const WM_WORK: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 3;
const WM_KEY_ACTION: u32 = WM_APP + 4;
const WM_NUMBER_ACTION: u32 = WM_APP + 5;
const HOTKEY_ID: i32 = 101;
const EDIT_ID: usize = 201;
const LIST_ID: usize = 202;
const MENU_ID: usize = 203;
const COPY_ID: usize = 204;
const OUTPUT_ID: usize = 205;
const OUTPUT_STATUS_ID: usize = 206;
const TIMER_SMOKE: usize = 301;
const MAX_ICONS: usize = 64;
// winresource::set_icon embeds the application icon with resource ID 1.
const APPLICATION_ICON_ID: usize = 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn application_icon(small: bool) -> HICON {
    let (width, height) = if small {
        (SM_CXSMICON, SM_CYSMICON)
    } else {
        (SM_CXICON, SM_CYICON)
    };
    // Shared resource handles live for the process lifetime; do not DestroyIcon them.
    let icon = LoadImageW(
        GetModuleHandleW(null()),
        APPLICATION_ICON_ID as *const u16,
        IMAGE_ICON,
        GetSystemMetrics(width),
        GetSystemMetrics(height),
        LR_SHARED,
    ) as HICON;
    if icon.is_null() {
        LoadIconW(null_mut(), IDI_APPLICATION)
    } else {
        icon
    }
}

#[derive(Clone)]
enum Page {
    Home,
    Menu,
    Plugins,
    Plugin(String),
    Settings,
}
#[derive(Clone)]
enum Action {
    Launch(Box<SearchEntry>),
    Home,
    Refresh,
    Plugins,
    Settings,
    Install,
    Detail(String),
    Toggle(String),
    Uninstall(String),
    SetHotkey(String),
    OpenData,
    OpenLog,
    ResetHistory,
    Autostart,
    Exit,
}
#[derive(Clone)]
struct Row {
    title: String,
    subtitle: String,
    badge: String,
    action: Action,
}
enum Event {
    Icon(String, usize),
    Refreshed(Cache, Vec<String>),
    Installed(Result<Manifest>),
    Uninstalled(String, Result<()>),
    CommandFinished(u64, Result<crate::command::CommandResult>),
}

struct ResultPanel {
    title: String,
    metadata: String,
    output: String,
    copy: Option<String>,
    terminal: bool,
    error: bool,
}

struct State {
    gesture: Option<crate::gesture::Gesture>,
    sessions: crate::interactive::Sessions,
    plugin_hotkeys: HashMap<i32, (String, String)>,
    paths: Paths,
    settings: Settings,
    cache: Cache,
    index: Vec<SearchEntry>,
    page: Page,
    rows: Vec<Row>,
    input: InputState,
    mode_exit_delete: Option<usize>,
    status: String,
    busy: bool,
    dialog: bool,
    suppress_edit: bool,
    composing: bool,
    hotkey_ok: bool,
    edit: HWND,
    list: HWND,
    button: HWND,
    menu_hover: bool,
    output: HWND,
    copy_button: HWND,
    output_status: HWND,
    panel: Option<ResultPanel>,
    copied: bool,
    command_directory: PathBuf,
    submitted_command: String,
    command_running: bool,
    command_id: u64,
    command_result: Option<Result<crate::command::CommandResult>>,
    font: HFONT,
    small_font: HFONT,
    search_font: HFONT,
    output_font: HFONT,
    background: HBRUSH,
    scale: f64,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    icons: HashMap<String, HICON>,
    icon_tx: Option<Sender<String>>,
    tray: NOTIFYICONDATAW,
    previous_window: HWND,
    monitor: HMONITOR,
    taskbar_message: u32,
    trace: bool,
}

unsafe fn trace(hwnd: HWND, event: &str) {
    let s = state(hwnd);
    if s.is_null() || !(*s).trace {
        return;
    }
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open((*s).paths.root.join("window-trace.log"))
    {
        let _ = writeln!(
            file,
            "{:?} {event} visible={} foreground={:?}",
            std::time::SystemTime::now(),
            IsWindowVisible(hwnd),
            GetForegroundWindow()
        );
    }
}

fn instance_name(paths: &Paths) -> String {
    let mut hasher = DefaultHasher::new();
    paths
        .root
        .canonicalize()
        .unwrap_or_else(|_| paths.root.clone())
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    format!("ptools.window.{:016x}", hasher.finish())
}

pub fn is_running(paths: &Paths) -> bool {
    unsafe { !FindWindowW(wide(&instance_name(paths)).as_ptr(), null()).is_null() }
}

pub fn error_box(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("ptools").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

unsafe fn state(hwnd: HWND) -> *mut State {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State
}
unsafe fn px(s: *mut State, n: i32) -> i32 {
    (n as f64 * (*s).scale).round() as i32
}

pub fn run(
    paths: Paths,
    settings: Settings,
    mut cache: Cache,
    hidden: bool,
    smoke: bool,
) -> Result<()> {
    unsafe {
        let class_name = wide(&instance_name(&paths));
        let existing = FindWindowW(class_name.as_ptr(), null());
        if !existing.is_null() {
            let mut pid = 0;
            GetWindowThreadProcessId(existing, &mut pid);
            AllowSetForegroundWindow(pid);
            PostMessageW(existing, WM_SHOW, 0, 0);
            return Ok(());
        }
        let mutex_name = wide(&format!("Local\\{}", instance_name(&paths)));
        let mutex = CreateMutexW(null(), 0, mutex_name.as_ptr());
        if mutex.is_null() {
            return Err("无法创建单实例锁".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(mutex);
            return Err("ptools 正在启动，请稍后重试".into());
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let (plugins, errors) = discover_plugins(&paths.plugins);
        cache
            .plugins
            .retain(|id, _| plugins.iter().any(|p| &p.manifest.id == id));
        let index = build_index(&cache, &settings);
        let (tx, rx) = mpsc::channel();
        let background = CreateSolidBrush(theme::BACKGROUND);
        let mut app = Box::new(State {
            gesture: None,
            sessions: Default::default(),
            plugin_hotkeys: HashMap::new(),
            paths,
            settings,
            cache,
            index,
            page: Page::Home,
            rows: vec![],
            input: InputState::default(),
            mode_exit_delete: None,
            status: if errors.is_empty() {
                "输入应用或系统功能 · 拼音 / 首字母也可以".into()
            } else {
                format!("{} 个插件加载失败，输入“设置”查看日志", errors.len())
            },
            busy: false,
            dialog: false,
            suppress_edit: false,
            composing: false,
            hotkey_ok: false,
            edit: null_mut(),
            list: null_mut(),
            button: null_mut(),
            menu_hover: false,
            output: null_mut(),
            copy_button: null_mut(),
            output_status: null_mut(),
            panel: None,
            copied: false,
            command_directory: crate::command::working_directory(),
            submitted_command: String::new(),
            command_running: false,
            command_id: 0,
            command_result: None,
            font: null_mut(),
            small_font: null_mut(),
            search_font: null_mut(),
            output_font: null_mut(),
            background,
            scale: GetDpiForSystem() as f64 / 96.0,
            tx,
            rx,
            icons: HashMap::new(),
            icon_tx: None,
            tray: zeroed(),
            previous_window: null_mut(),
            monitor: null_mut(),
            taskbar_message: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            trace: std::env::var_os("PTOOLS_TRACE").is_some(),
        });
        let module = GetModuleHandleW(null());
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: background,
            lpszClassName: class_name.as_ptr(),
            hIcon: application_icon(false),
            hIconSm: application_icon(true),
            ..zeroed()
        };
        if RegisterClassExW(&class) == 0 {
            CloseHandle(mutex);
            return Err(format!("注册窗口失败：{}", GetLastError()));
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            wide("ptools").as_ptr(),
            WS_POPUP | WS_BORDER | WS_CLIPCHILDREN,
            100,
            100,
            (660.0 * app.scale) as i32,
            (120.0 * app.scale) as i32,
            null_mut(),
            null_mut(),
            module,
            (&mut *app as *mut State).cast(),
        );
        if hwnd.is_null() {
            CloseHandle(mutex);
            return Err(format!("创建窗口失败：{}", GetLastError()));
        }
        let rounded: u32 = 3;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&rounded as *const u32).cast(),
            4,
        );
        app.hotkey_ok = register_hotkey(hwnd, &app.settings.hotkey);
        if !app.hotkey_ok {
            app.status = format!("{} 已被占用，请进入“设置”更换快捷键", app.settings.hotkey);
        }
        setup_tray(hwnd);
        sync_plugin_hotkeys(hwnd);
        rebuild(hwnd);
        if !hidden {
            show(hwnd);
        }
        start_refresh(hwnd);
        if smoke {
            SetTimer(hwnd, TIMER_SMOKE, 5000, None);
        }
        let mut message: MSG = zeroed();
        loop {
            let status = GetMessageW(&mut message, null_mut(), 0, 0);
            if status <= 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        for (_, icon) in app.icons.drain() {
            DestroyIcon(icon);
        }
        for font in [app.font, app.small_font, app.search_font, app.output_font] {
            if !font.is_null() {
                DeleteObject(font);
            }
        }
        DeleteObject(background);
        CloseHandle(mutex);
        Ok(())
    }
}

unsafe fn register_hotkey(hwnd: HWND, name: &str) -> bool {
    let modifiers = match name {
        "Ctrl+Alt+Space" => MOD_CONTROL | MOD_ALT,
        "Ctrl+Space" => MOD_CONTROL,
        _ => MOD_ALT,
    };
    RegisterHotKey(hwnd, HOTKEY_ID, modifiers | MOD_NOREPEAT, VK_SPACE as u32) != 0
}

unsafe fn sync_plugin_hotkeys(hwnd: HWND) {
    let s = state(hwnd);
    for (id, _) in (*s).plugin_hotkeys.drain() {
        UnregisterHotKey(hwnd, id);
    }
    let (plugins, _) = discover_plugins(&(*s).paths.plugins);
    let gesture_enabled = plugins.iter().any(|plugin| {
        plugin.manifest.id == "capture"
            && !(*s).settings.disabled_plugins.contains("capture")
            && plugin
                .manifest
                .actions
                .iter()
                .any(|action| action.id == "quick")
            && interactive_executable(plugin, "quick").is_ok()
    });
    if !gesture_enabled {
        (*s).gesture = None;
    } else if (*s).gesture.is_none() {
        match crate::gesture::Gesture::start(hwnd) {
            Ok(gesture) => (*s).gesture = Some(gesture),
            Err(error) => (*s).status = error,
        }
    }
    let mut id = 1000;
    for plugin in plugins {
        if (*s).settings.disabled_plugins.contains(&plugin.manifest.id) {
            continue;
        }
        for action in &plugin.manifest.actions {
            let key = format!("{}:{}", plugin.manifest.id, action.id);
            let value = (*s)
                .settings
                .plugin_hotkeys
                .get(&key)
                .map(String::as_str)
                .or(action.hotkey.as_deref());
            if let Some(value) = value.filter(|s| !s.is_empty()) {
                if let Some((mods, vk)) = crate::interactive::hotkey(value) {
                    if RegisterHotKey(hwnd, id, mods, vk) != 0 {
                        (*s).plugin_hotkeys
                            .insert(id, (plugin.manifest.id.clone(), action.id.clone()));
                    } else {
                        (*s).status =
                            format!("{} 的快捷键 {value} 被占用；仍可通过搜索打开", action.title);
                    }
                }
                id += 1;
            }
        }
    }
}

unsafe fn fonts(hwnd: HWND) {
    let s = state(hwnd);
    let mut old = Vec::new();
    for (slot, size, weight, family) in [
        (&raw mut (*s).font, 15, 400, "Microsoft YaHei UI"),
        (&raw mut (*s).small_font, 11, 400, "Microsoft YaHei UI"),
        (&raw mut (*s).search_font, 21, 400, "Microsoft YaHei UI"),
        (&raw mut (*s).output_font, 14, 400, "Consolas"),
    ] {
        old.push(*slot);
        *slot = CreateFontW(
            -px(s, size),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide(family).as_ptr(),
        );
    }
    SendMessageW((*s).edit, WM_SETFONT, (*s).search_font as usize, 1);
    SendMessageW((*s).list, WM_SETFONT, (*s).font as usize, 1);
    SendMessageW((*s).button, WM_SETFONT, (*s).font as usize, 1);
    SendMessageW((*s).copy_button, WM_SETFONT, (*s).font as usize, 1);
    SendMessageW((*s).output_status, WM_SETFONT, (*s).small_font as usize, 1);
    SendMessageW(
        (*s).output,
        WM_SETFONT,
        if (*s).panel.as_ref().is_some_and(|panel| panel.terminal) {
            (*s).output_font as usize
        } else {
            (*s).search_font as usize
        },
        1,
    );
    SendMessageW((*s).list, LB_SETITEMHEIGHT, 0, px(s, 58) as isize);
    for font in old {
        if !font.is_null() {
            DeleteObject(font);
        }
    }
}

unsafe fn setup_tray(hwnd: HWND) {
    let s = state(hwnd);
    let mut tray: NOTIFYICONDATAW = zeroed();
    tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray.hWnd = hwnd;
    tray.uID = 1;
    tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    tray.uCallbackMessage = WM_TRAY;
    tray.hIcon = application_icon(true);
    let tip = wide(&format!("ptools · {}", (*s).settings.hotkey));
    tray.szTip[..tip.len().min(128)].copy_from_slice(&tip[..tip.len().min(128)]);
    Shell_NotifyIconW(NIM_ADD, &tray);
    (*s).tray = tray;
}

unsafe fn show(hwnd: HWND) {
    trace(hwnd, "show");
    let s = state(hwnd);
    let foreground = GetForegroundWindow();
    if foreground != hwnd {
        (*s).previous_window = foreground;
    }
    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    (*s).monitor = monitor;
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(monitor, &mut info);
    let work = info.rcWork;
    let width = px(s, 660).min(work.right - work.left - 20);
    let mut rect: RECT = zeroed();
    GetWindowRect(hwnd, &mut rect);
    let height = rect.bottom - rect.top;
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        work.left + (work.right - work.left - width) / 2,
        work.top + (work.bottom - work.top - height) / 2,
        width,
        height,
        SWP_SHOWWINDOW,
    );
    trace(hwnd, "show-positioned");
    layout(hwnd);
    trace(hwnd, "show-layout");
    SetForegroundWindow(hwnd);
    trace(hwnd, "show-foreground");
    SetFocus((*s).edit);
    trace(hwnd, "show-focused");
    SendMessageW((*s).edit, EM_SETSEL, 0, -1);
    trace(hwnd, "show-finished");
}

unsafe fn hide(hwnd: HWND, restore: bool) {
    trace(hwnd, if restore { "hide-restore" } else { "hide" });
    let previous = (*state(hwnd)).previous_window;
    ShowWindow(hwnd, SW_HIDE);
    if restore && !previous.is_null() && IsWindow(previous) != 0 {
        SetForegroundWindow(previous);
    }
}

unsafe fn layout(hwnd: HWND) {
    let s = state(hwnd);
    let monitor = if (*s).monitor.is_null() {
        MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
    } else {
        (*s).monitor
    };
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) == 0 {
        (*s).monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW((*s).monitor, &mut info);
    }
    let work = info.rcWork;
    let count = (*s).rows.len().min(7) as i32;
    let list_height = if let Some(panel) = &(*s).panel {
        px(
            s,
            if panel.terminal {
                118 + panel.output.lines().count().clamp(4, 10) as i32 * 18
            } else {
                150
            },
        )
    } else if count == 0 {
        px(s, 52)
    } else {
        px(s, 58) * count
    };
    let width = px(s, 660).min(work.right - work.left - 20).max(1);
    let height = (px(s, 116) + list_height + 2)
        .min(work.bottom - work.top - 20)
        .max(1);
    SetWindowPos(
        hwnd,
        null_mut(),
        work.left + (work.right - work.left - width) / 2,
        work.top + (work.bottom - work.top - height) / 2,
        width,
        height,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    position_children(hwnd);
    let quick = (*s).panel.is_some();
    ShowWindow(
        (*s).list,
        if !quick && count > 0 {
            SW_SHOWNA
        } else {
            SW_HIDE
        },
    );
    for control in [(*s).output, (*s).copy_button, (*s).output_status] {
        ShowWindow(control, if quick { SW_SHOWNA } else { SW_HIDE });
    }
    InvalidateRect(hwnd, null(), 0);
}

unsafe fn position_children(hwnd: HWND) {
    let s = state(hwnd);
    let mut rect: RECT = zeroed();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right;
    let list_height = (rect.bottom - px(s, 116)).max(0);
    MoveWindow(
        (*s).edit,
        px(s, 56),
        px(s, 24),
        width - px(s, 116),
        px(s, 34),
        1,
    );
    MoveWindow(
        (*s).button,
        width - px(s, 48),
        px(s, 22),
        px(s, 32),
        px(s, 34),
        1,
    );
    MoveWindow(
        (*s).list,
        px(s, 10),
        px(s, 80),
        width - px(s, 20),
        list_height,
        1,
    );
    MoveWindow(
        (*s).copy_button,
        width - px(s, 126),
        px(s, 88),
        px(s, 100),
        px(s, 32),
        1,
    );
    MoveWindow(
        (*s).output_status,
        px(s, 26),
        px(s, 119),
        width - px(s, 54),
        px(s, 22),
        1,
    );
    MoveWindow(
        (*s).output,
        px(s, 28),
        px(s, 155),
        (width - px(s, 58)).max(1),
        (rect.bottom - px(s, 218)).max(1),
        1,
    );
    if let Some(panel) = &(*s).panel {
        let mut rect: RECT = zeroed();
        GetClientRect((*s).output, &mut rect);
        let columns = (rect.right as f64 / (7.8 * (*s).scale)).max(1.0) as usize;
        let horizontal = panel.terminal
            && panel.output.lines().any(|line| {
                line.chars()
                    .map(|c| if c.is_ascii() { 1 } else { 2 })
                    .sum::<usize>()
                    > columns
            });
        let visible_lines = ((rect.bottom - if horizontal { px(s, 18) } else { 0 })
            / px(s, 18).max(1))
        .max(1) as usize;
        ShowScrollBar((*s).output, SB_HORZ, i32::from(horizontal));
        ShowScrollBar(
            (*s).output,
            SB_VERT,
            i32::from(panel.terminal && panel.output.lines().count() > visible_lines),
        );
    }
}

fn command(title: &str, subtitle: &str, badge: &str, action: Action) -> Row {
    Row {
        title: title.into(),
        subtitle: subtitle.into(),
        badge: badge.into(),
        action,
    }
}

fn entry_badge(entry: &Entry) -> &'static str {
    match &entry.target {
        Target::System { id } => match system_command(id).map(|command| &command.location) {
            Some(SystemLocation::Uri(uri)) if uri.starts_with("shell:") => "目录",
            Some(SystemLocation::Uri(_)) => "设置",
            _ => "系统",
        },
        _ => "应用",
    }
}

fn result_panel(state: &State) -> Option<ResultPanel> {
    if !matches!(state.page, Page::Home) {
        return None;
    }
    match state.input.mode {
        InputMode::Search => None,
        InputMode::Calculator => {
            let expression = state.input.text.trim();
            let (output, copy, error) = if expression.is_empty() {
                ("例如：(18 + 2) * 3".into(), None, false)
            } else {
                match crate::calculator::evaluate(expression) {
                    Ok(value) => (value.clone(), Some(value), false),
                    Err(error) => (error, None, true),
                }
            };
            Some(ResultPanel {
                title: if error {
                    "表达式有误"
                } else {
                    "计算结果"
                }
                .into(),
                metadata: "实时计算 · 支持括号、四则运算、幂和取余".into(),
                output,
                copy,
                terminal: false,
                error,
            })
        }
        InputMode::Terminal => {
            let command = state.input.text.trim();
            let mut panel = ResultPanel {
                title: "终端命令".into(),
                metadata: format!("PowerShell · {}", state.command_directory.display()),
                output: "输入命令后按 Enter 执行\n例如：Get-Date 或 ipconfig".into(),
                copy: None,
                terminal: true,
                error: false,
            };
            if state.command_running {
                panel.title = "正在执行".into();
                panel.output = if command == state.submitted_command {
                    "命令正在运行，结果将在这里显示…".into()
                } else {
                    "上一条命令仍在运行，完成后可执行这条命令。".into()
                };
                panel.metadata = "PowerShell · 正在执行".into();
            } else if !command.is_empty() && command == state.submitted_command {
                match &state.command_result {
                    Some(Ok(result)) => {
                        panel.error =
                            result.timed_out || result.truncated || result.exit_code != Some(0);
                        panel.title = if result.timed_out {
                            "执行超时"
                        } else if result.truncated {
                            "输出已截断"
                        } else if panel.error {
                            "命令失败"
                        } else {
                            "命令输出"
                        }
                        .into();
                        let exit = result.exit_code.map_or("—".into(), |code| code.to_string());
                        panel.metadata = format!(
                            "PowerShell · 退出码 {exit} · {} ms{}",
                            result.elapsed.as_millis(),
                            if result.truncated {
                                " · 输出已截断"
                            } else {
                                ""
                            },
                        );
                        panel.output = if result.output.is_empty() {
                            "命令没有输出。".into()
                        } else {
                            result.output.clone()
                        };
                        if result.timed_out {
                            panel.output.push_str("\n\n命令超过 30 秒，已停止运行。");
                        } else if result.truncated {
                            panel
                                .output
                                .push_str("\n\n输出超过 1 MiB，命令已停止运行。");
                        }
                        panel.copy = (!result.output.is_empty()).then(|| result.output.clone());
                    }
                    Some(Err(error)) => {
                        panel.title = "执行失败".into();
                        panel.output = error.clone();
                        panel.copy = Some(error.clone());
                        panel.error = true;
                    }
                    None => {}
                }
            }
            Some(panel)
        }
    }
}

unsafe fn update_result_panel(hwnd: HWND) {
    let s = state(hwnd);
    let panel = result_panel(&*s);
    if let Some(panel) = &panel {
        let text = panel.output.replace("\r\n", "\n").replace('\n', "\r\n");
        // A refresh event should not reset a user's selection or output scroll position.
        if (*s)
            .panel
            .as_ref()
            .is_none_or(|old| old.output != panel.output)
        {
            SetWindowTextW((*s).output, wide(&text).as_ptr());
        }
        SetWindowTextW((*s).output_status, wide(&panel.metadata).as_ptr());
        EnableWindow((*s).copy_button, i32::from(panel.copy.is_some()));
        SetWindowTextW(
            (*s).copy_button,
            wide(if (*s).copied {
                "已复制 ✓"
            } else {
                "复制结果"
            })
            .as_ptr(),
        );
        SendMessageW(
            (*s).output,
            WM_SETFONT,
            if panel.terminal {
                (*s).output_font
            } else if panel.copy.is_some() {
                (*s).search_font
            } else {
                (*s).font
            } as usize,
            1,
        );
    }
    (*s).panel = panel;
}

unsafe fn rebuild(hwnd: HWND) {
    let s = state(hwnd);
    update_result_panel(hwnd);
    if (*s).panel.is_some() {
        (*s).rows.clear();
        SendMessageW((*s).list, LB_RESETCONTENT, 0, 0);
        layout(hwnd);
        return;
    }
    let query = (*s).input.text.trim().to_lowercase();
    let mut rows = Vec::new();
    match (*s).page.clone() {
        Page::Home => {
            if !query.is_empty() {
                for i in search(&(*s).index, &query, &(*s).settings, 50) {
                    let hit = (&(*s).index)[i].clone();
                    rows.push(Row {
                        title: hit.entry.title.clone(),
                        subtitle: hit.entry.subtitle.clone(),
                        badge: entry_badge(&hit.entry).into(),
                        action: Action::Launch(Box::new(hit)),
                    });
                }
            } else {
                for i in search(&(*s).index, "", &(*s).settings, 50) {
                    let hit = (&(*s).index)[i].clone();
                    if (*s).settings.usage.contains_key(&hit.key) {
                        rows.push(Row {
                            title: hit.entry.title.clone(),
                            subtitle: "最近使用".into(),
                            badge: entry_badge(&hit.entry).into(),
                            action: Action::Launch(Box::new(hit)),
                        });
                    }
                    if rows.len() >= 5 {
                        break;
                    }
                }
            }
            for (keywords, row) in [
                (
                    "插件 plugins chajian cj",
                    command(
                        "插件管理",
                        "安装、启用、停用和卸载本地插件",
                        "管理",
                        Action::Plugins,
                    ),
                ),
                (
                    "设置 settings shezhi sz",
                    command(
                        "设置",
                        "快捷键、开机启动和数据目录",
                        "设置",
                        Action::Settings,
                    ),
                ),
                (
                    "刷新 refresh shuaxin sx",
                    command(
                        "刷新应用索引",
                        "重新发现应用；刷新期间仍可搜索",
                        "刷新",
                        Action::Refresh,
                    ),
                ),
                (
                    "退出 quit exit tuichu tc",
                    command(
                        "退出 ptools",
                        "结束程序并释放全局快捷键",
                        "退出",
                        Action::Exit,
                    ),
                ),
            ] {
                if !query.is_empty()
                    && (keywords.split_whitespace().any(|x| x.starts_with(&query))
                        || row.title.contains(&query))
                {
                    rows.insert(0, row);
                }
            }
        }
        Page::Menu => {
            for row in [
                command("返回搜索", "搜索应用、系统功能或拼音", "←", Action::Home),
                command("刷新应用索引", "重新发现已安装的应用", "↻", Action::Refresh),
                command("插件管理", "安装与管理你的本地插件", "+", Action::Plugins),
                command("设置", "快捷键、开机启动和数据目录", "⚙", Action::Settings),
                command("退出 ptools", "结束程序并释放快捷键", "×", Action::Exit),
            ] {
                if query.is_empty() || row.title.to_lowercase().contains(&query) {
                    rows.push(row);
                }
            }
        }
        Page::Plugins => {
            rows.push(command(
                "安装本地插件",
                "选择 .ptplugin / .zip 或插件目录中的 plugin.json",
                "+",
                Action::Install,
            ));
            let (plugins, errors) = discover_plugins(&(*s).paths.plugins);
            for plugin in plugins {
                let id = plugin.manifest.id.clone();
                let enabled = !(*s).settings.disabled_plugins.contains(&id);
                let count = (*s).cache.plugins.get(&id).map_or(0, Vec::len);
                let row = command(
                    &plugin.manifest.name,
                    &format!(
                        "{} · {} · {} 个入口",
                        plugin.manifest.version,
                        if enabled { "已启用" } else { "已停用" },
                        count
                    ),
                    "插件",
                    Action::Detail(id),
                );
                if query.is_empty() || row.title.to_lowercase().contains(&query) {
                    rows.push(row);
                }
            }
            if !errors.is_empty() {
                (*s).status = format!("{} 个插件无效；打开数据目录检查清单", errors.len());
            }
            rows.push(command("返回搜索", "Esc 也可返回", "←", Action::Home));
        }
        Page::Plugin(id) => {
            if let Ok(plugin) = load_plugin(&(*s).paths.plugins.join(&id)) {
                let disabled = (*s).settings.disabled_plugins.contains(&id);
                rows.push(command(
                    if disabled {
                        "启用插件"
                    } else {
                        "停用插件"
                    },
                    &plugin.manifest.description,
                    "插件",
                    Action::Toggle(id.clone()),
                ));
                rows.push(command(
                    "卸载插件",
                    "删除插件文件和搜索入口，其他插件不受影响",
                    "卸载",
                    Action::Uninstall(id),
                ));
            }
            rows.push(command(
                "返回插件管理",
                "Esc 也可返回",
                "←",
                Action::Plugins,
            ));
        }
        Page::Settings => {
            for hotkey in ["Alt+Space", "Ctrl+Alt+Space", "Ctrl+Space"] {
                let current = (*s).settings.hotkey == hotkey;
                rows.push(command(
                    &format!("快捷键：{hotkey}"),
                    if current {
                        if (*s).hotkey_ok {
                            "当前快捷键"
                        } else {
                            "当前快捷键注册失败，请选择其他组合"
                        }
                    } else {
                        "按回车切换；冲突时保留原设置"
                    },
                    if current { "当前" } else { "切换" },
                    Action::SetHotkey(hotkey.into()),
                ));
            }
            rows.push(command(
                if autostart_enabled() {
                    "关闭开机启动"
                } else {
                    "开启开机启动"
                },
                "仅当前用户，无需管理员权限",
                "启动",
                Action::Autostart,
            ));
            rows.push(command(
                "打开数据目录",
                &format!(
                    "{} · {}",
                    if (*s).paths.portable {
                        "便携模式"
                    } else {
                        "用户模式"
                    },
                    (*s).paths.root.display()
                ),
                "目录",
                Action::OpenData,
            ));
            rows.push(command(
                "查看刷新日志",
                "记录最近一次刷新中的警告和错误",
                "日志",
                Action::OpenLog,
            ));
            rows.push(command(
                "清空使用记录",
                "重置最近使用与使用频率排序",
                "清空",
                Action::ResetHistory,
            ));
            rows.push(command("返回搜索", "Esc 也可返回", "←", Action::Home));
        }
    }
    (*s).rows = rows;
    SendMessageW((*s).list, WM_SETREDRAW, 0, 0);
    SendMessageW((*s).list, LB_RESETCONTENT, 0, 0);
    let titles: Vec<_> = (*s).rows.iter().map(|r| wide(&r.title)).collect();
    for title in titles {
        SendMessageW((*s).list, LB_ADDSTRING, 0, title.as_ptr() as isize);
    }
    if !(*s).rows.is_empty() {
        SendMessageW((*s).list, LB_SETCURSEL, 0, 0);
    }
    SendMessageW((*s).list, WM_SETREDRAW, 1, 0);
    layout(hwnd);
    InvalidateRect((*s).list, null(), 1);
}

unsafe fn sync_input_control(hwnd: HWND) {
    let s = state(hwnd);
    let cue = match (*s).input.mode {
        InputMode::Search => "搜索应用，= 计算，> 运行命令",
        InputMode::Calculator => "输入算式，如 1 + 2 * 3",
        InputMode::Terminal => "输入命令，如 Get-Date",
    };
    (*s).suppress_edit = true;
    SetWindowTextW((*s).edit, wide(&(*s).input.text).as_ptr());
    SendMessageW((*s).edit, EM_SETCUEBANNER, 1, wide(cue).as_ptr() as isize);
    SendMessageW((*s).edit, EM_SETSEL, usize::MAX, -1);
    (*s).suppress_edit = false;
}

unsafe fn leave_input_mode(hwnd: HWND) -> bool {
    let s = state(hwnd);
    if !(*s).input.leave_mode() {
        return false;
    }
    (*s).copied = false;
    sync_input_control(hwnd);
    rebuild(hwnd);
    if IsWindowVisible(hwnd) != 0 {
        SetFocus((*s).edit);
    }
    true
}

unsafe fn navigate(hwnd: HWND, page: Page) {
    let s = state(hwnd);
    let cue = match &page {
        Page::Home => "搜索应用，= 计算，> 运行命令",
        Page::Menu => "菜单",
        Page::Plugins => "插件管理",
        Page::Plugin(_) => "插件设置",
        Page::Settings => "设置",
    };
    (*s).page = page;
    (*s).input.reset();
    (*s).mode_exit_delete = None;
    (*s).suppress_edit = true;
    SetWindowTextW((*s).edit, wide("").as_ptr());
    SendMessageW((*s).edit, EM_SETCUEBANNER, 1, wide(cue).as_ptr() as isize);
    (*s).suppress_edit = false;
    rebuild(hwnd);
    InvalidateRect((*s).button, null(), 0);
    // A hidden child must never receive focus: SetFocus can activate its hidden parent
    // and steal focus back from the application we have just launched.
    if IsWindowVisible(hwnd) != 0 {
        SetFocus((*s).edit);
    }
}

unsafe fn start_refresh(hwnd: HWND) {
    let s = state(hwnd);
    if (*s).busy {
        return;
    }
    (*s).busy = true;
    (*s).status = "正在后台刷新应用…".into();
    let paths = (*s).paths.clone();
    let settings = (*s).settings.clone();
    let old = (*s).cache.clone();
    let tx = (*s).tx.clone();
    let window = hwnd as usize;
    std::thread::spawn(move || {
        let (cache, mut errors) = refresh(&paths, &settings, &old);
        if let Err(e) = write_json(&paths.cache(), &cache) {
            errors.push(e);
        }
        if let Err(e) = write_json(&paths.root.join("last-refresh.json"), &errors) {
            errors.push(e);
        }
        let _ = tx.send(Event::Refreshed(cache, errors));
        PostMessageW(window as HWND, WM_WORK, 0, 0);
    });
    InvalidateRect(hwnd, null(), 0);
}

unsafe fn save_settings(hwnd: HWND) -> bool {
    let s = state(hwnd);
    match write_json(&(*s).paths.settings(), &(*s).settings) {
        Ok(()) => true,
        Err(e) => {
            alert(hwnd, &e, false);
            false
        }
    }
}

unsafe fn alert(hwnd: HWND, text: &str, confirm: bool) -> bool {
    let s = state(hwnd);
    (*s).dialog = true;
    let result = MessageBoxW(
        hwnd,
        wide(text).as_ptr(),
        wide("ptools").as_ptr(),
        if confirm {
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2
        } else {
            MB_OK | MB_ICONINFORMATION
        },
    );
    (*s).dialog = false;
    SetFocus((*s).edit);
    result == IDYES
}

unsafe fn shell_open(hwnd: HWND, target: &str) -> bool {
    let result = ShellExecuteW(
        hwnd,
        wide("open").as_ptr(),
        wide(target).as_ptr(),
        null(),
        null(),
        SW_SHOWNORMAL,
    );
    if (result as isize) <= 32 {
        alert(
            hwnd,
            &format!(
                "无法打开目标（错误 {}）。目标可能不可用或已被移除，请刷新索引。",
                result as isize
            ),
            false,
        );
        false
    } else {
        true
    }
}

#[allow(clippy::needless_borrow)]
unsafe fn execute(hwnd: HWND) {
    let s = state(hwnd);
    if (*s).panel.is_some() {
        match (*s).input.mode {
            InputMode::Calculator => copy_result(hwnd),
            InputMode::Terminal => start_command(hwnd),
            InputMode::Search => {}
        }
        return;
    }
    let selected = SendMessageW((*s).list, LB_GETCURSEL, 0, 0);
    if selected < 0 {
        return;
    }
    let Some(row) = (&(*s).rows).get(selected as usize).cloned() else {
        return;
    };
    act(hwnd, row.action);
}

unsafe fn copy_result(hwnd: HWND) {
    let s = state(hwnd);
    let Some(text) = (*s)
        .panel
        .as_ref()
        .and_then(|panel| panel.copy.as_ref())
        .cloned()
    else {
        return;
    };
    let value = wide(&text);
    let memory = GlobalAlloc(GMEM_MOVEABLE, value.len() * size_of::<u16>());
    let error = if memory.is_null() {
        Some("无法分配剪贴板内存")
    } else {
        let buffer = GlobalLock(memory);
        if buffer.is_null() {
            GlobalFree(memory);
            Some("无法访问剪贴板内存")
        } else {
            std::ptr::copy_nonoverlapping(value.as_ptr(), buffer.cast(), value.len());
            GlobalUnlock(memory);
            if OpenClipboard(hwnd) == 0 {
                GlobalFree(memory);
                Some("剪贴板正被占用，请再试一次")
            } else {
                let copied = EmptyClipboard() != 0 && !SetClipboardData(13, memory).is_null();
                CloseClipboard();
                if copied {
                    None
                } else {
                    GlobalFree(memory);
                    Some("复制失败，请再试一次")
                }
            }
        }
    };
    if let Some(error) = error {
        SetWindowTextW((*s).output_status, wide(error).as_ptr());
    } else {
        (*s).copied = true;
        SetWindowTextW((*s).copy_button, wide("已复制 ✓").as_ptr());
    }
    InvalidateRect(hwnd, null(), 0);
}

unsafe fn start_command(hwnd: HWND) {
    let s = state(hwnd);
    if (*s).command_running {
        return;
    }
    if (*s).input.mode != InputMode::Terminal {
        return;
    }
    let command = (*s).input.text.trim();
    if command.is_empty() {
        return;
    }
    let command = command.to_owned();
    (*s).submitted_command = command.clone();
    (*s).command_result = None;
    (*s).command_running = true;
    (*s).copied = false;
    (*s).command_id = (*s).command_id.wrapping_add(1);
    let request = (*s).command_id;
    let directory = (*s).command_directory.clone();
    let tx = (*s).tx.clone();
    let window = hwnd as usize;
    std::thread::spawn(move || {
        let result = crate::command::run(&command, &directory);
        let _ = tx.send(Event::CommandFinished(request, result));
        PostMessageW(window as HWND, WM_WORK, 0, 0);
    });
    rebuild(hwnd);
}

unsafe fn act(hwnd: HWND, action: Action) {
    let s = state(hwnd);
    match action {
        Action::Launch(item) => {
            if let Target::Plugin { action } = &item.entry.target {
                hide(hwnd, false);
                if let Err(e) =
                    (*s).sessions
                        .invoke(&(*s).paths, &(*s).settings, &item.plugin_id, action)
                {
                    alert(hwnd, &e, false);
                }
                return;
            }
            let target = match &item.entry.target {
                Target::Path { path } => path.replace('/', "\\"),
                Target::Url { url } => url.clone(),
                Target::App { app_id } => format!("shell:AppsFolder\\{app_id}"),
                Target::System { id } => {
                    let resolved = system_command(id)
                        .ok_or_else(|| "未知的 Windows 系统入口".to_string())
                        .and_then(SystemCommand::resolve);
                    match resolved {
                        Ok(target) => target,
                        Err(error) => {
                            alert(hwnd, &error, false);
                            return;
                        }
                    }
                }
                Target::Plugin { .. } => unreachable!(),
            };
            // Hide before launch so focus loss from the launched application cannot restore a stale foreground window.
            hide(hwnd, false);
            if shell_open(hwnd, &target) {
                let usage = (*s).settings.usage.entry(item.key).or_default();
                usage.count = usage.count.saturating_add(1);
                usage.last_used = now();
                save_settings(hwnd);
                navigate(hwnd, Page::Home);
            } else {
                show(hwnd);
            }
        }
        Action::Home => navigate(hwnd, Page::Home),
        Action::Plugins => navigate(hwnd, Page::Plugins),
        Action::Settings => navigate(hwnd, Page::Settings),
        Action::Detail(id) => navigate(hwnd, Page::Plugin(id)),
        Action::Refresh => {
            start_refresh(hwnd);
            navigate(hwnd, Page::Home);
        }
        Action::OpenData => {
            shell_open(hwnd, &(*s).paths.root.to_string_lossy());
        }
        Action::OpenLog => {
            let log = (*s).paths.root.join("last-refresh.json");
            if log.exists() {
                shell_open(hwnd, &log.to_string_lossy());
            } else {
                alert(hwnd, "还没有刷新日志。", false);
            }
        }
        Action::SetHotkey(name) => {
            if (*s).settings.hotkey == name && (*s).hotkey_ok {
                return;
            }
            let old = (*s).settings.hotkey.clone();
            UnregisterHotKey(hwnd, HOTKEY_ID);
            if register_hotkey(hwnd, &name) {
                (*s).settings.hotkey = name;
                (*s).hotkey_ok = true;
                if !save_settings(hwnd) {
                    UnregisterHotKey(hwnd, HOTKEY_ID);
                    (*s).hotkey_ok = register_hotkey(hwnd, &old);
                    (*s).settings.hotkey = old;
                }
                (*s).status = format!("{} 显示 / 隐藏", (*s).settings.hotkey);
                Shell_NotifyIconW(NIM_DELETE, &(*s).tray);
                setup_tray(hwnd);
            } else {
                (*s).hotkey_ok = register_hotkey(hwnd, &old);
                alert(hwnd, "这个快捷键已被其他程序占用，请选择其他组合。", false);
            }
            rebuild(hwnd);
        }
        Action::Autostart => {
            if let Err(e) = set_autostart(!autostart_enabled(), &(*s).paths) {
                alert(hwnd, &e, false);
            }
            rebuild(hwnd);
        }
        Action::ResetHistory => {
            if alert(hwnd, "清空最近使用和使用频率记录？", true) {
                (*s).settings.usage.clear();
                save_settings(hwnd);
                (*s).status = "使用记录已清空".into();
                rebuild(hwnd);
            }
        }
        Action::Toggle(id) => {
            if (*s).busy {
                alert(hwnd, "正在刷新或安装插件，请完成后重试。", false);
                return;
            }
            if !(*s).settings.disabled_plugins.remove(&id) {
                (*s).sessions.stop(&id);
                (*s).settings.disabled_plugins.insert(id);
            }
            sync_plugin_hotkeys(hwnd);
            save_settings(hwnd);
            (*s).index = build_index(&(*s).cache, &(*s).settings);
            rebuild(hwnd);
            start_refresh(hwnd);
        }
        Action::Install => {
            if (*s).busy {
                alert(hwnd, "正在刷新或安装插件，请完成后重试。", false);
                return;
            }
            let mut filename = vec![0u16; 32768];
            let filter = wide("ptools 插件\0*.ptplugin;*.zip;plugin.json\0所有文件\0*.*\0");
            let mut ofn: OPENFILENAMEW = zeroed();
            ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
            ofn.hwndOwner = hwnd;
            ofn.lpstrFilter = filter.as_ptr();
            ofn.lpstrFile = filename.as_mut_ptr();
            ofn.nMaxFile = filename.len() as u32;
            let title = wide("选择插件包，或插件目录中的 plugin.json");
            ofn.lpstrTitle = title.as_ptr();
            ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
            (*s).dialog = true;
            let selected = GetOpenFileNameW(&mut ofn);
            (*s).dialog = false;
            if selected == 0 {
                return;
            }
            let path = PathBuf::from(String::from_utf16_lossy(
                &filename[..filename.iter().position(|x| *x == 0).unwrap_or(0)],
            ));
            if !alert(
                hwnd,
                &format!(
                    "安装插件：{}\n\n原生插件可以访问当前用户的文件和系统。请只安装你信任的插件。",
                    path.display()
                ),
                true,
            ) {
                return;
            }
            (*s).busy = true;
            (*s).status = "正在安装插件…".into();
            let root = (*s).paths.plugins.clone();
            let tx = (*s).tx.clone();
            let window = hwnd as usize;
            std::thread::spawn(move || {
                let result = install_plugin(&path, &root);
                let _ = tx.send(Event::Installed(result));
                PostMessageW(window as HWND, WM_WORK, 0, 0);
            });
            InvalidateRect(hwnd, null(), 0);
        }
        Action::Uninstall(id) => {
            if (*s).busy {
                alert(hwnd, "正在刷新或安装插件，请完成后重试。", false);
                return;
            }
            if !alert(hwnd, "卸载此插件并移除它提供的全部搜索入口？", true) {
                return;
            }
            (*s).busy = true;
            (*s).sessions.stop(&id);
            (*s).status = "正在卸载插件…".into();
            let root = (*s).paths.plugins.clone();
            let tx = (*s).tx.clone();
            let window = hwnd as usize;
            std::thread::spawn(move || {
                let result = uninstall_plugin(&root, &id);
                let _ = tx.send(Event::Uninstalled(id, result));
                PostMessageW(window as HWND, WM_WORK, 0, 0);
            });
        }
        Action::Exit => {
            DestroyWindow(hwnd);
        }
    }
}

unsafe fn autostart_enabled() -> bool {
    let mut key = null_mut();
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
        0,
        KEY_QUERY_VALUE,
        &mut key,
    ) != 0
    {
        return false;
    }
    let result = RegQueryValueExW(
        key,
        wide("ptools").as_ptr(),
        null(),
        null_mut(),
        null_mut(),
        null_mut(),
    );
    RegCloseKey(key);
    result == 0
}

unsafe fn set_autostart(enable: bool, paths: &Paths) -> Result<()> {
    let mut key = null_mut();
    let code = RegCreateKeyExW(
        HKEY_CURRENT_USER,
        wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),
        0,
        null(),
        0,
        KEY_SET_VALUE,
        null(),
        &mut key,
        null_mut(),
    );
    if code != 0 {
        return Err(format!("无法修改开机启动：{code}"));
    }
    let code = if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = wide(&format!(
            "\"{}\" --hidden --data-dir \"{}\"",
            exe.display(),
            paths.root.display()
        ));
        RegSetValueExW(
            key,
            wide("ptools").as_ptr(),
            0,
            REG_SZ,
            command.as_ptr().cast(),
            (command.len() * 2) as u32,
        )
    } else {
        RegDeleteValueW(key, wide("ptools").as_ptr())
    };
    RegCloseKey(key);
    if code != 0 {
        Err(format!("修改开机启动失败：{code}"))
    } else {
        Ok(())
    }
}

unsafe fn handle_events(hwnd: HWND) {
    let s = state(hwnd);
    let events: Vec<_> = (*s).rx.try_iter().collect();
    for event in events {
        if !matches!(&event, Event::Icon(_, _) | Event::CommandFinished(_, _)) {
            (*s).busy = false;
        }
        match event {
            Event::CommandFinished(request, result) => {
                if request == (*s).command_id {
                    (*s).command_running = false;
                    (*s).command_result = Some(result);
                    (*s).copied = false;
                    rebuild(hwnd);
                }
            }
            Event::Icon(path, icon) => {
                (*s).icons.insert(path, icon as HICON);
                InvalidateRect((*s).list, null(), 0);
            }
            Event::Refreshed(cache, errors) => {
                (*s).cache = cache;
                (*s).index = build_index(&(*s).cache, &(*s).settings);
                (*s).status = if !(*s).hotkey_ok {
                    format!("{} 已被占用，输入“设置”更换", (*s).settings.hotkey)
                } else if errors.is_empty() {
                    format!("{} 个启动入口 · 输入“插件”或“设置”管理", (*s).index.len())
                } else {
                    format!(
                        "{} 个入口 · {} 条刷新警告，设置中查看日志",
                        (*s).index.len(),
                        errors.len()
                    )
                };
                rebuild(hwnd);
                sync_plugin_hotkeys(hwnd);
            }
            Event::Installed(result) => match result {
                Ok(manifest) => {
                    (*s).settings.disabled_plugins.remove(&manifest.id);
                    save_settings(hwnd);
                    sync_plugin_hotkeys(hwnd);
                    navigate(hwnd, Page::Plugins);
                    start_refresh(hwnd);
                }
                Err(e) => {
                    (*s).status = "安装失败".into();
                    alert(hwnd, &e, false);
                    rebuild(hwnd);
                }
            },
            Event::Uninstalled(id, result) => match result {
                Ok(()) => {
                    (*s).cache.plugins.remove(&id);
                    (*s).settings.disabled_plugins.remove(&id);
                    (*s).settings
                        .usage
                        .retain(|key, _| !key.starts_with(&format!("{id}:")));
                    if let Err(e) = write_json(&(*s).paths.cache(), &(*s).cache) {
                        alert(hwnd, &e, false);
                    }
                    save_settings(hwnd);
                    sync_plugin_hotkeys(hwnd);
                    (*s).index = build_index(&(*s).cache, &(*s).settings);
                    (*s).status = "插件已卸载".into();
                    navigate(hwnd, Page::Plugins);
                }
                Err(e) => {
                    alert(hwnd, &e, false);
                    rebuild(hwnd);
                }
            },
        }
    }
}

unsafe fn draw_text(dc: HDC, text: &str, mut rect: RECT, font: HFONT, rgb: u32, flags: u32) {
    let old = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, rgb);
    let text = wide(text);
    DrawTextW(
        dc,
        text.as_ptr(),
        (text.len() - 1) as i32,
        &mut rect,
        DT_NOPREFIX | flags,
    );
    SelectObject(dc, old);
}

unsafe fn app_icon(hwnd: HWND, s: *mut State, row: &Row) -> HICON {
    let Action::Launch(entry) = &row.action else {
        return null_mut();
    };
    let path = match &entry.entry.target {
        Target::Path { path } => path.clone(),
        Target::System { id } => {
            let Some(command) = system_command(id) else {
                return null_mut();
            };
            if !matches!(command.location, SystemLocation::File(_)) {
                return null_mut();
            }
            let Ok(path) = command.resolve() else {
                return null_mut();
            };
            path
        }
        _ => return null_mut(),
    };
    if let Some(icon) = (*s).icons.get(&path) {
        return *icon;
    }
    if (*s).icons.len() >= MAX_ICONS {
        return null_mut();
    }
    // Network paths use the generic icon. Shell extensions are queried away from the UI thread.
    if path.starts_with("\\\\") {
        return null_mut();
    }
    (*s).icons.insert(path.clone(), null_mut());
    if let Some(tx) = &(*s).icon_tx
        && tx.send(path.clone()).is_ok()
    {
        return null_mut();
    }
    let (tx, rx) = mpsc::channel::<String>();
    let events = (*s).tx.clone();
    let window = hwnd as usize;
    let _ = tx.send(path.clone());
    (*s).icon_tx = Some(tx);
    std::thread::spawn(move || {
        let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        while let Ok(path) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
            let mut info: SHFILEINFOW = zeroed();
            let result = SHGetFileInfoW(
                wide(&path.replace('/', "\\")).as_ptr(),
                0,
                &mut info,
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            );
            let icon = if result == 0 { null_mut() } else { info.hIcon };
            if events.send(Event::Icon(path, icon as usize)).is_err() {
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                break;
            }
            PostMessageW(window as HWND, WM_WORK, 0, 0);
        }
        if initialized {
            CoUninitialize();
        }
    });
    null_mut()
}

#[allow(clippy::needless_borrow)]
unsafe fn draw_row(hwnd: HWND, draw: &DRAWITEMSTRUCT) {
    let s = state(hwnd);
    let Some(row) = (&(*s).rows).get(draw.itemID as usize).cloned() else {
        return;
    };
    let selected = draw.itemState & ODS_SELECTED != 0;
    let rect = draw.rcItem;
    let brush = CreateSolidBrush(if selected {
        theme::SELECTION
    } else {
        theme::BACKGROUND
    });
    FillRect(draw.hDC, &rect, (*s).background);
    if selected {
        let old_brush = SelectObject(draw.hDC, brush);
        let old_pen = SelectObject(draw.hDC, GetStockObject(NULL_PEN));
        RoundRect(
            draw.hDC,
            rect.left,
            rect.top + px(s, 2),
            rect.right,
            rect.bottom - px(s, 2),
            px(s, 3),
            px(s, 3),
        );
        SelectObject(draw.hDC, old_pen);
        SelectObject(draw.hDC, old_brush);
    }
    DeleteObject(brush);
    if selected {
        theme::fill(
            draw.hDC,
            &RECT {
                left: rect.left,
                top: rect.top + px(s, 6),
                right: rect.left + px(s, 3),
                bottom: rect.bottom - px(s, 6),
            },
            theme::ACCENT,
        );
    }
    let (top, visible) = visible_rows((*s).list);
    let slot = draw.itemID as isize - top;
    if slot >= 0 && slot < visible {
        let number_rect = RECT {
            left: rect.left + px(s, 6),
            right: rect.left + px(s, 30),
            ..rect
        };
        draw_text(
            draw.hDC,
            &(slot + 1).to_string(),
            number_rect,
            (*s).small_font,
            theme::MUTED,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
    let icon = app_icon(hwnd, s, &row);
    if !icon.is_null() {
        DrawIconEx(
            draw.hDC,
            rect.left + px(s, 38),
            rect.top + px(s, 13),
            icon,
            px(s, 30),
            px(s, 30),
            0,
            null_mut(),
            DI_NORMAL,
        );
    } else {
        let mut badge = rect;
        badge.left += px(s, 34);
        badge.right = badge.left + px(s, 40);
        let glyph = match row.action {
            Action::Launch(_) => "↗",
            _ => &row.badge,
        };
        draw_text(
            draw.hDC,
            glyph,
            badge,
            (*s).font,
            theme::ACCENT,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
    let title_rect = RECT {
        left: rect.left + px(s, 82),
        top: rect.top + px(s, 8),
        right: rect.right - px(s, 44),
        bottom: rect.top + px(s, 31),
    };
    draw_text(
        draw.hDC,
        &row.title,
        title_rect,
        (*s).font,
        theme::TEXT,
        DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let subtitle_rect = RECT {
        left: title_rect.left,
        top: rect.top + px(s, 33),
        right: rect.right - px(s, 25),
        bottom: rect.bottom - px(s, 5),
    };
    draw_text(
        draw.hDC,
        &row.subtitle,
        subtitle_rect,
        (*s).small_font,
        theme::MUTED,
        DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    if selected {
        let arrow = RECT {
            left: rect.right - px(s, 35),
            top: rect.top,
            right: rect.right - px(s, 10),
            bottom: rect.bottom,
        };
        draw_text(
            draw.hDC,
            "↵",
            arrow,
            (*s).font,
            theme::ACCENT,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );
    }
}

#[allow(clippy::needless_borrow)]
unsafe fn paint(hwnd: HWND) {
    let s = state(hwnd);
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    FillRect(dc, &ps.rcPaint, (*s).background);
    theme::frame(dc, &client, theme::BORDER);
    theme::fill(
        dc,
        &RECT {
            left: px(s, 16),
            top: client.bottom - px(s, 37),
            right: client.right - px(s, 16),
            bottom: client.bottom - px(s, 36),
        },
        theme::BORDER,
    );
    if let Some(panel) = &(*s).panel {
        draw_text(
            dc,
            if panel.terminal { ">" } else { "=" },
            RECT {
                left: px(s, 20),
                top: px(s, 24),
                right: px(s, 46),
                bottom: px(s, 58),
            },
            (*s).search_font,
            theme::ACCENT,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER,
        );
    } else {
        let pen = CreatePen(PS_SOLID, px(s, 2).max(1), theme::ACCENT);
        let old_pen = SelectObject(dc, pen);
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Ellipse(dc, px(s, 23), px(s, 30), px(s, 39), px(s, 46));
        MoveToEx(dc, px(s, 37), px(s, 44), null_mut());
        LineTo(dc, px(s, 44), px(s, 51));
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pen);
    }
    let line_brush = CreateSolidBrush(theme::BORDER);
    let line = RECT {
        left: px(s, 16),
        top: px(s, 75),
        right: client.right - px(s, 16),
        bottom: px(s, 76),
    };
    FillRect(dc, &line, line_brush);
    DeleteObject(line_brush);
    if let Some(panel) = &(*s).panel {
        let rect = RECT {
            left: px(s, 16),
            top: px(s, 84),
            right: client.right - px(s, 16),
            bottom: client.bottom - px(s, 48),
        };
        theme::fill(dc, &rect, theme::SURFACE);
        theme::frame(dc, &rect, theme::BORDER);
        draw_text(
            dc,
            &panel.title,
            RECT {
                left: px(s, 26),
                top: px(s, 90),
                right: client.right - px(s, 140),
                bottom: px(s, 117),
            },
            (*s).font,
            if panel.error {
                theme::DANGER
            } else {
                theme::TEXT
            },
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
    } else if (*s).rows.is_empty() {
        let empty = if !matches!((*s).page, Page::Home) {
            "这里暂时没有内容"
        } else if !(&(*s).input.text).is_empty() {
            "没有找到应用 · 输入“刷新”更新索引"
        } else if (*s).index.is_empty() {
            if (*s).busy {
                "正在发现你的应用…"
            } else {
                "还没有启动入口 · 输入“插件”安装插件"
            }
        } else {
            "试试设备管理器、显示设置或应用名称。"
        };
        let rect = RECT {
            left: px(s, 24),
            top: px(s, 84),
            right: client.right - px(s, 24),
            bottom: client.bottom - px(s, 40),
        };
        draw_text(
            dc,
            empty,
            rect,
            (*s).font,
            theme::MUTED,
            DT_SINGLELINE | DT_VCENTER,
        );
    }
    let status = if let Some(panel) = &(*s).panel {
        if panel.terminal {
            "终端命令".into()
        } else {
            "计算器 · 实时计算".into()
        }
    } else {
        (*s).status.clone()
    };
    let footer = RECT {
        left: px(s, 20),
        top: client.bottom - px(s, 31),
        right: client.right - px(s, 335),
        bottom: client.bottom - px(s, 8),
    };
    draw_text(
        dc,
        &status,
        footer,
        (*s).small_font,
        theme::MUTED,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    let hints = RECT {
        left: client.right - px(s, 328),
        right: client.right - px(s, 18),
        ..footer
    };
    draw_text(
        dc,
        if let Some(panel) = &(*s).panel {
            if panel.terminal {
                "↵ 执行   Ctrl+Shift+C 复制   Esc 返回"
            } else {
                "↵ 复制   Ctrl+Shift+C 复制   Esc 返回"
            }
        } else {
            "Alt+数字 / ↑↓ / Ctrl+J/K 选择   ↵ 打开   Esc 返回"
        },
        hints,
        (*s).small_font,
        theme::MUTED,
        DT_SINGLELINE | DT_RIGHT | DT_VCENTER,
    );
    EndPaint(hwnd, &ps);
}

unsafe fn menu(hwnd: HWND) {
    let s = state(hwnd);
    let page = if matches!((*s).page, Page::Menu) {
        Page::Home
    } else {
        Page::Menu
    };
    navigate(hwnd, page);
}

unsafe fn tray_menu(hwnd: HWND) {
    let s = state(hwnd);
    let popup = CreatePopupMenu();
    if popup.is_null() {
        return;
    }
    AppendMenuW(
        popup,
        MF_STRING
            | if autostart_enabled() {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
        2,
        wide("自启动").as_ptr(),
    );
    AppendMenuW(popup, MF_STRING, 1, wide("关闭").as_ptr());
    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    (*s).dialog = true;
    SetForegroundWindow(hwnd);
    let selected = TrackPopupMenu(
        popup,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(popup);
    (*s).dialog = false;
    // Let the shell dismiss the menu reliably on subsequent tray clicks.
    PostMessageW(hwnd, WM_NULL, 0, 0);
    match selected {
        1 => act(hwnd, Action::Exit),
        2 => act(hwnd, Action::Autostart),
        _ => {}
    }
}

unsafe fn draw_menu_button(hwnd: HWND, draw: &DRAWITEMSTRUCT) {
    let s = state(hwnd);
    let active = matches!((*s).page, Page::Menu);
    FillRect(draw.hDC, &draw.rcItem, (*s).background);
    let brush = CreateSolidBrush(if active || draw.itemState & ODS_SELECTED != 0 {
        theme::SELECTION
    } else if (*s).menu_hover {
        theme::RAISED
    } else {
        theme::BACKGROUND
    });
    let old_brush = SelectObject(draw.hDC, brush);
    let old_pen = SelectObject(draw.hDC, GetStockObject(NULL_PEN));
    let r = draw.rcItem;
    RoundRect(
        draw.hDC,
        r.left,
        r.top,
        r.right,
        r.bottom,
        px(s, 10),
        px(s, 10),
    );
    SelectObject(draw.hDC, old_brush);
    DeleteObject(brush);
    let ink = CreateSolidBrush(theme::ACCENT);
    let old_brush = SelectObject(draw.hDC, ink);
    let cy = (r.top + r.bottom) / 2;
    for offset in [-7, 0, 7] {
        let cx = (r.left + r.right) / 2 + px(s, offset);
        Ellipse(
            draw.hDC,
            cx - px(s, 2),
            cy - px(s, 2),
            cx + px(s, 2),
            cy + px(s, 2),
        );
    }
    SelectObject(draw.hDC, old_brush);
    SelectObject(draw.hDC, old_pen);
    DeleteObject(ink);
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    if number_shortcut(parent as HWND, msg, w, l) {
        return 0;
    }
    let s = state(parent as HWND);
    if !s.is_null() {
        if msg == WM_MOUSEMOVE && !(*s).menu_hover {
            (*s).menu_hover = true;
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            InvalidateRect(hwnd, null(), 0);
        } else if msg == WM_MOUSELEAVE {
            (*s).menu_hover = false;
            InvalidateRect(hwnd, null(), 0);
        }
    }
    DefSubclassProc(hwnd, msg, w, l)
}

unsafe fn visible_rows(list: HWND) -> (isize, isize) {
    let top = SendMessageW(list, LB_GETTOPINDEX, 0, 0).max(0);
    let height = SendMessageW(list, LB_GETITEMHEIGHT, 0, 0).max(1);
    let count = SendMessageW(list, LB_GETCOUNT, 0, 0).max(0);
    let mut client: RECT = zeroed();
    GetClientRect(list, &mut client);
    let visible = ((client.bottom - client.top) as isize / height)
        .min((count - top).max(0))
        .clamp(0, 9);
    (top, visible)
}

unsafe fn number_shortcut(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> bool {
    let s = state(hwnd);
    if s.is_null()
        || (*s).composing
        || !(0x31..=0x39).contains(&w)
        || l & (1 << 29) == 0
        || GetKeyState(VK_CONTROL as i32) < 0
        || GetKeyState(VK_SHIFT as i32) < 0
    {
        return false;
    }
    match msg {
        WM_SYSKEYDOWN => {
            // Ignore key-repeat; a numbered shortcut only changes the selection.
            if l & (1 << 30) == 0 {
                PostMessageW(hwnd, WM_NUMBER_ACTION, w - 0x31, 0);
            }
            true
        }
        WM_SYSCHAR | WM_SYSKEYUP => true,
        _ => false,
    }
}

unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    if number_shortcut(parent as HWND, msg, w, l) {
        return 0;
    }
    let scrolling = matches!(
        msg,
        WM_VSCROLL | WM_MOUSEWHEEL | LB_SETTOPINDEX | LB_SETCURSEL
    );
    let top = if scrolling {
        SendMessageW(hwnd, LB_GETTOPINDEX, 0, 0)
    } else {
        0
    };
    let result = DefSubclassProc(hwnd, msg, w, l);
    if scrolling && top != SendMessageW(hwnd, LB_GETTOPINDEX, 0, 0) {
        // Native scrolling can reuse old pixels; redraw labels against the new top row.
        InvalidateRect(hwnd, null(), 0);
    }
    result
}

unsafe fn result_copy_shortcut(hwnd: HWND, msg: u32, w: WPARAM) -> bool {
    let s = state(hwnd);
    if (*s).panel.is_none()
        || (*s).composing
        || GetKeyState(VK_CONTROL as i32) >= 0
        || GetKeyState(VK_SHIFT as i32) >= 0
        || GetKeyState(VK_MENU as i32) < 0
    {
        return false;
    }
    if msg == WM_KEYDOWN && w == 0x43 {
        copy_result(hwnd);
        true
    } else {
        // TranslateMessage emits Ctrl+C after WM_KEYDOWN; keep the input's
        // native copy action from replacing the result just put on the clipboard.
        msg == WM_CHAR && w == 3
    }
}

unsafe fn focus_result_control(hwnd: HWND) {
    let s = state(hwnd);
    let mut controls = vec![(*s).edit, (*s).output];
    if IsWindowEnabled((*s).copy_button) != 0 {
        controls.push((*s).copy_button);
    }
    let current = controls
        .iter()
        .position(|&control| control == GetFocus())
        .unwrap_or(0);
    let next = if GetKeyState(VK_SHIFT as i32) < 0 {
        (current + controls.len() - 1) % controls.len()
    } else {
        (current + 1) % controls.len()
    };
    SetFocus(controls[next]);
}

unsafe extern "system" fn result_control_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    let parent = parent as HWND;
    if result_copy_shortcut(parent, msg, w) {
        return 0;
    }
    match msg {
        WM_KEYDOWN if w == VK_ESCAPE as usize => {
            if l & (1 << 30) == 0 {
                PostMessageW(parent, WM_KEY_ACTION, w, 0);
            }
            return 0;
        }
        WM_KEYDOWN if w == VK_TAB as usize => {
            focus_result_control(parent);
            return 0;
        }
        WM_KEYDOWN if w == VK_RETURN as usize => {
            if l & (1 << 30) == 0 {
                copy_result(parent);
            }
            return 0;
        }
        WM_KEYDOWN if w == 0x41 && GetKeyState(VK_CONTROL as i32) < 0 => {
            SendMessageW(hwnd, EM_SETSEL, 0, -1);
            return 0;
        }
        WM_CHAR if [VK_RETURN as usize, VK_TAB as usize, VK_ESCAPE as usize].contains(&w) => {
            return 0;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, w, l)
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    let parent = parent as HWND;
    let s = state(parent);
    if s.is_null() {
        return DefSubclassProc(hwnd, msg, w, l);
    }
    if result_copy_shortcut(parent, msg, w) {
        return 0;
    }
    if number_shortcut(parent, msg, w, l) {
        return 0;
    }
    match msg {
        WM_IME_STARTCOMPOSITION => {
            (*s).composing = true;
        }
        WM_IME_ENDCOMPOSITION => {
            (*s).composing = false;
        }
        WM_KEYUP if (*s).mode_exit_delete == Some(w) => {
            (*s).mode_exit_delete = None;
        }
        WM_KEYDOWN if !(*s).composing => {
            // A held Enter cannot rerun a command; a held Esc only goes back once.
            if [VK_RETURN as usize, VK_ESCAPE as usize].contains(&w) && l & (1 << 30) != 0 {
                return 0;
            }
            if [VK_BACK as usize, VK_DELETE as usize].contains(&w) {
                if l & (1 << 30) == 0 {
                    (*s).mode_exit_delete = None;
                } else if (*s).mode_exit_delete == Some(w) {
                    return 0;
                }
                let empty_tool = {
                    let input = &(*s).input;
                    input.mode != InputMode::Search && input.text.is_empty()
                };
                if empty_tool {
                    leave_input_mode(parent);
                    // Swallow the translated Backspace character and held-key
                    // repeats, which otherwise delete the restored search text.
                    (*s).mode_exit_delete = Some(w);
                    return 0;
                }
            }
            if GetKeyState(VK_CONTROL as i32) < 0
                && GetKeyState(VK_MENU as i32) >= 0
                && [0x4A, 0x4B].contains(&w)
            {
                let direction = if w == 0x4A { VK_DOWN } else { VK_UP };
                PostMessageW(parent, WM_KEY_ACTION, direction as usize, 0);
                return 0;
            }
            if [
                VK_RETURN as usize,
                VK_ESCAPE as usize,
                VK_UP as usize,
                VK_DOWN as usize,
                VK_TAB as usize,
            ]
            .contains(&w)
            {
                PostMessageW(parent, WM_KEY_ACTION, w, 0);
                return 0;
            }
            if w == 0x41 && GetKeyState(VK_CONTROL as i32) < 0 {
                SendMessageW(hwnd, EM_SETSEL, 0, -1);
                return 0;
            }
        }
        WM_CHAR
            if !(*s).composing
                && [VK_BACK as usize, 0x7F].contains(&w)
                && (*s).mode_exit_delete == Some(VK_BACK as usize) =>
        {
            return 0;
        }
        WM_CHAR
            if !(*s).composing
                // TranslateMessage produces LF / VT for Ctrl+J / Ctrl+K.
                && [VK_RETURN as usize, VK_ESCAPE as usize, VK_TAB as usize, 0x0A, 0x0B]
                    .contains(&w) =>
        {
            return 0;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, w, l)
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let s = state(hwnd);
    if s.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    if number_shortcut(hwnd, msg, w, l) {
        return 0;
    }
    if msg == (*s).taskbar_message {
        setup_tray(hwnd);
        return 0;
    }
    match msg {
        WM_CREATE => {
            let module = GetModuleHandleW(null());
            (*s).edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                56,
                24,
                500,
                34,
                hwnd,
                EDIT_ID as HMENU,
                module,
                null(),
            );
            (*s).list = CreateWindowExW(
                0,
                wide("LISTBOX").as_ptr(),
                null(),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_VSCROLL
                    | LBS_NOTIFY as u32
                    | LBS_OWNERDRAWFIXED as u32
                    | LBS_HASSTRINGS as u32
                    | LBS_NOINTEGRALHEIGHT as u32,
                10,
                80,
                640,
                100,
                hwnd,
                LIST_ID as HMENU,
                module,
                null(),
            );
            (*s).button = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide("菜单").as_ptr(),
                WS_CHILD | WS_VISIBLE | BS_OWNERDRAW as u32,
                610,
                22,
                32,
                34,
                hwnd,
                MENU_ID as HMENU,
                module,
                null(),
            );
            (*s).copy_button = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide("复制结果").as_ptr(),
                WS_CHILD | WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                0,
                100,
                32,
                hwnd,
                COPY_ID as HMENU,
                module,
                null(),
            );
            (*s).output = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WS_HSCROLL
                    | ES_MULTILINE as u32
                    | ES_READONLY as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_AUTOHSCROLL as u32
                    | ES_NOHIDESEL as u32,
                0,
                0,
                500,
                180,
                hwnd,
                OUTPUT_ID as HMENU,
                module,
                null(),
            );
            (*s).output_status = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("").as_ptr(),
                // SS_LEFTNOWORDWRAP | SS_NOPREFIX | SS_ENDELLIPSIS.
                WS_CHILD | 0x000C | 0x0080 | 0x4000,
                0,
                0,
                500,
                22,
                hwnd,
                OUTPUT_STATUS_ID as HMENU,
                module,
                null(),
            );
            SetWindowSubclass((*s).edit, Some(edit_proc), 1, hwnd as usize);
            SetWindowSubclass((*s).list, Some(list_proc), 1, hwnd as usize);
            SetWindowSubclass((*s).button, Some(button_proc), 1, hwnd as usize);
            SetWindowSubclass((*s).output, Some(result_control_proc), 1, hwnd as usize);
            SetWindowSubclass(
                (*s).copy_button,
                Some(result_control_proc),
                1,
                hwnd as usize,
            );
            SendMessageW((*s).edit, EM_SETLIMITTEXT, 4096, 0);
            SendMessageW(
                (*s).edit,
                EM_SETCUEBANNER,
                1,
                wide("搜索应用，= 计算，> 运行命令").as_ptr() as isize,
            );
            theme::control((*s).edit);
            theme::control((*s).list);
            theme::control((*s).output);
            theme::button((*s).copy_button, theme::PRIMARY_BUTTON);
            fonts(hwnd);
            return 0;
        }
        WM_COMMAND => {
            let id = w & 0xffff;
            let event = (w >> 16) & 0xffff;
            if id == EDIT_ID && event == EN_CHANGE as usize && !(*s).suppress_edit {
                let mut text =
                    vec![0u16; GetWindowTextLengthW((*s).edit).clamp(0, 4096) as usize + 1];
                let len = GetWindowTextW((*s).edit, text.as_mut_ptr(), text.len() as i32);
                let text = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                if matches!((*s).page, Page::Home) {
                    if (*s).input.update(text) {
                        sync_input_control(hwnd);
                    }
                } else {
                    (*s).input.text = text;
                }
                (*s).copied = false;
                rebuild(hwnd);
            } else if id == LIST_ID && event == LBN_DBLCLK as usize {
                execute(hwnd);
            } else if id == LIST_ID && event == LBN_SELCHANGE as usize {
                SetFocus((*s).edit);
            } else if id == MENU_ID && event == BN_CLICKED as usize {
                menu(hwnd);
            } else if id == COPY_ID && event == BN_CLICKED as usize {
                copy_result(hwnd);
            }
            return 0;
        }
        WM_NUMBER_ACTION => {
            let (top, visible) = visible_rows((*s).list);
            if !(*s).composing && w < visible as usize {
                SendMessageW((*s).list, LB_SETCURSEL, top as usize + w, 0);
                InvalidateRect((*s).list, null(), 0);
                SetFocus((*s).edit);
            }
            return 0;
        }
        WM_KEY_ACTION => {
            match w as u16 {
                VK_RETURN => execute(hwnd),
                VK_ESCAPE => match (*s).page.clone() {
                    Page::Home => {
                        if !leave_input_mode(hwnd) {
                            navigate(hwnd, Page::Home);
                            hide(hwnd, true);
                        }
                    }
                    Page::Plugin(_) => navigate(hwnd, Page::Plugins),
                    _ => navigate(hwnd, Page::Home),
                },
                VK_UP | VK_DOWN => {
                    let count = (*s).rows.len() as isize;
                    if count > 0 {
                        let current = SendMessageW((*s).list, LB_GETCURSEL, 0, 0).max(0);
                        let next =
                            (current + if w as u16 == VK_DOWN { 1 } else { -1 }).rem_euclid(count);
                        SendMessageW((*s).list, LB_SETCURSEL, next as usize, 0);
                        InvalidateRect((*s).list, null(), 0);
                    }
                }
                VK_TAB => {
                    if (*s).panel.is_some() {
                        focus_result_control(hwnd);
                    } else {
                        menu(hwnd);
                    }
                }
                _ => {}
            }
            return 0;
        }
        crate::gesture::WM_QUICK_PIN => {
            while let Some(region) = (*s).gesture.as_ref().and_then(|gesture| gesture.region()) {
                hide(hwnd, false);
                if let Err(e) = (*s).sessions.invoke_region(
                    &(*s).paths,
                    &(*s).settings,
                    "capture",
                    "quick",
                    Some(region),
                ) {
                    alert(hwnd, &e, false);
                }
            }
            return 0;
        }
        WM_HOTKEY => {
            if let Some((id, action)) = (*s).plugin_hotkeys.get(&(w as i32)).cloned() {
                let modifiers = l as u32 & 0xffff;
                let key = (l as u32 >> 16) & 0xffff;
                if IsWindowVisible(hwnd) != 0
                    && modifiers == MOD_ALT
                    && (0x31..=0x39).contains(&key)
                {
                    PostMessageW(hwnd, WM_NUMBER_ACTION, (key - 0x31) as usize, 0);
                    return 0;
                }
                hide(hwnd, false);
                if let Err(e) = (*s)
                    .sessions
                    .invoke(&(*s).paths, &(*s).settings, &id, &action)
                {
                    alert(hwnd, &e, false);
                }
                return 0;
            }
            trace(hwnd, "hotkey");
            if IsWindowVisible(hwnd) != 0 {
                hide(hwnd, true);
            } else {
                navigate(hwnd, Page::Home);
                show(hwnd);
            }
            return 0;
        }
        WM_SHOW => {
            trace(hwnd, "second-instance-show");
            navigate(hwnd, Page::Home);
            show(hwnd);
            return 0;
        }
        WM_ACTIVATE if w & 0xffff == WA_INACTIVE as usize => {
            if !(*s).dialog {
                hide(hwnd, false);
            }
            return 0;
        }
        WM_WORK => {
            handle_events(hwnd);
            return 0;
        }
        WM_TRAY => {
            match l as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    navigate(hwnd, Page::Home);
                    show(hwnd);
                }
                WM_RBUTTONUP => tray_menu(hwnd),
                _ => {}
            }
            return 0;
        }
        WM_MEASUREITEM => {
            let measure = &mut *(l as *mut MEASUREITEMSTRUCT);
            measure.itemHeight = px(s, 58) as u32;
            return 1;
        }
        WM_DRAWITEM => {
            let draw = &*(l as *const DRAWITEMSTRUCT);
            if draw.CtlID == MENU_ID as u32 {
                draw_menu_button(hwnd, draw);
            } else {
                draw_row(hwnd, draw);
            }
            return 1;
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORSTATIC => {
            if l as HWND == (*s).output || l as HWND == (*s).output_status {
                SetBkColor(w as HDC, theme::SURFACE);
                SetTextColor(
                    w as HDC,
                    if (*s).panel.as_ref().is_some_and(|panel| panel.error)
                        && (l as HWND == (*s).output_status
                            || !(*s).panel.as_ref().unwrap().terminal)
                    {
                        theme::DANGER
                    } else if l as HWND == (*s).output_status {
                        theme::MUTED
                    } else {
                        theme::TEXT
                    },
                );
                return theme::surface_brush() as isize;
            }
            SetBkColor(w as HDC, theme::BACKGROUND);
            SetTextColor(w as HDC, theme::TEXT);
            return (*s).background as isize;
        }
        WM_SIZE => {
            if !(*s).edit.is_null() {
                position_children(hwnd);
            }
            return 0;
        }
        WM_PAINT => {
            paint(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_DPICHANGED => {
            (*s).scale = (w & 0xffff) as f64 / 96.0;
            fonts(hwnd);
            let rect = &*(l as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            layout(hwnd);
            return 0;
        }
        WM_TIMER if w == TIMER_SMOKE => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_CLOSE => {
            hide(hwnd, true);
            return 0;
        }
        WM_DESTROY => {
            (*s).gesture = None;
            for (id, _) in (*s).plugin_hotkeys.drain() {
                UnregisterHotKey(hwnd, id);
            }
            UnregisterHotKey(hwnd, HOTKEY_ID);
            Shell_NotifyIconW(NIM_DELETE, &(*s).tray);
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}
