#![allow(unsafe_op_in_unsafe_fn)]

use ptools_core::*;
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
        LibraryLoader::GetModuleHandleW,
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
const TIMER_SMOKE: usize = 301;
const MAX_ICONS: usize = 64;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn color(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
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
}

struct State {
    paths: Paths,
    settings: Settings,
    cache: Cache,
    index: Vec<SearchEntry>,
    page: Page,
    rows: Vec<Row>,
    query: String,
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
    font: HFONT,
    small_font: HFONT,
    search_font: HFONT,
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
        let background = CreateSolidBrush(color(250, 251, 253));
        let mut app = Box::new(State {
            paths,
            settings,
            cache,
            index,
            page: Page::Home,
            rows: vec![],
            query: String::new(),
            status: if errors.is_empty() {
                "输入应用名称 · 拼音 / 首字母也可以".into()
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
            font: null_mut(),
            small_font: null_mut(),
            search_font: null_mut(),
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
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
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
        let rounded: u32 = 2;
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
        rebuild(hwnd);
        if !hidden || !app.hotkey_ok {
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
        for font in [app.font, app.small_font, app.search_font] {
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

unsafe fn fonts(hwnd: HWND) {
    let s = state(hwnd);
    let mut old = Vec::new();
    for (slot, size, weight) in [
        (&raw mut (*s).font, 15, 400),
        (&raw mut (*s).small_font, 11, 400),
        (&raw mut (*s).search_font, 21, 400),
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
            wide("Microsoft YaHei UI").as_ptr(),
        );
    }
    SendMessageW((*s).edit, WM_SETFONT, (*s).search_font as usize, 1);
    SendMessageW((*s).list, WM_SETFONT, (*s).font as usize, 1);
    SendMessageW((*s).button, WM_SETFONT, (*s).font as usize, 1);
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
    tray.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
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
    let list_height = if count == 0 {
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
    ShowWindow((*s).list, if count > 0 { SW_SHOWNA } else { SW_HIDE });
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
}

fn command(title: &str, subtitle: &str, badge: &str, action: Action) -> Row {
    Row {
        title: title.into(),
        subtitle: subtitle.into(),
        badge: badge.into(),
        action,
    }
}

unsafe fn rebuild(hwnd: HWND) {
    let s = state(hwnd);
    let query = (*s).query.trim().to_lowercase();
    let mut rows = Vec::new();
    match (*s).page.clone() {
        Page::Home => {
            if !query.is_empty() {
                for i in search(&(*s).index, &query, &(*s).settings, 50) {
                    let hit = (&(*s).index)[i].clone();
                    rows.push(Row {
                        title: hit.entry.title.clone(),
                        subtitle: hit.entry.subtitle.clone(),
                        badge: "应用".into(),
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
                            badge: "应用".into(),
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
                        || row.title.contains(&query)
                        || query == ">")
                {
                    rows.insert(0, row);
                }
            }
        }
        Page::Menu => {
            for row in [
                command("返回搜索", "搜索应用、拼音或首字母", "←", Action::Home),
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

unsafe fn navigate(hwnd: HWND, page: Page) {
    let s = state(hwnd);
    let cue = match &page {
        Page::Home => "搜索应用、拼音或首字母",
        Page::Menu => "菜单",
        Page::Plugins => "插件管理",
        Page::Plugin(_) => "插件设置",
        Page::Settings => "设置",
    };
    (*s).page = page;
    (*s).query.clear();
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
                "无法打开目标（错误 {}）。应用可能已被移除，请刷新索引。",
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
    let selected = SendMessageW((*s).list, LB_GETCURSEL, 0, 0);
    if selected < 0 {
        return;
    }
    let Some(row) = (&(*s).rows).get(selected as usize).cloned() else {
        return;
    };
    act(hwnd, row.action);
}

unsafe fn act(hwnd: HWND, action: Action) {
    let s = state(hwnd);
    match action {
        Action::Launch(item) => {
            let target = match &item.entry.target {
                Target::Path { path } => path.replace('/', "\\"),
                Target::Url { url } => url.clone(),
                Target::App { app_id } => format!("shell:AppsFolder\\{app_id}"),
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
                (*s).settings.disabled_plugins.insert(id);
            }
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
        if !matches!(&event, Event::Icon(_, _)) {
            (*s).busy = false;
        }
        match event {
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
                    format!("{} 个应用入口 · 输入“插件”或“设置”管理", (*s).index.len())
                } else {
                    format!(
                        "{} 个入口 · {} 条刷新警告，设置中查看日志",
                        (*s).index.len(),
                        errors.len()
                    )
                };
                rebuild(hwnd);
            }
            Event::Installed(result) => match result {
                Ok(manifest) => {
                    (*s).settings.disabled_plugins.remove(&manifest.id);
                    save_settings(hwnd);
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
    let Target::Path { path } = &entry.entry.target else {
        return null_mut();
    };
    if let Some(icon) = (*s).icons.get(path) {
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
        color(232, 241, 255)
    } else {
        color(250, 251, 253)
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
            px(s, 12),
            px(s, 12),
        );
        SelectObject(draw.hDC, old_pen);
        SelectObject(draw.hDC, old_brush);
    }
    DeleteObject(brush);
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
            color(125, 135, 152),
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
            color(57, 104, 190),
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
        color(30, 39, 56),
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
        color(110, 120, 137),
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
            color(57, 104, 190),
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
    let pen = CreatePen(PS_SOLID, px(s, 2).max(1), color(71, 109, 176));
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    Ellipse(dc, px(s, 23), px(s, 30), px(s, 39), px(s, 46));
    MoveToEx(dc, px(s, 37), px(s, 44), null_mut());
    LineTo(dc, px(s, 44), px(s, 51));
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    let line_brush = CreateSolidBrush(color(226, 232, 241));
    let line = RECT {
        left: px(s, 16),
        top: px(s, 75),
        right: client.right - px(s, 16),
        bottom: px(s, 76),
    };
    FillRect(dc, &line, line_brush);
    DeleteObject(line_brush);
    if (*s).rows.is_empty() {
        let empty = if !matches!((*s).page, Page::Home) {
            "这里暂时没有内容"
        } else if !(&(*s).query).is_empty() {
            "没有找到应用 · 输入“刷新”更新索引"
        } else if (*s).index.is_empty() {
            if (*s).busy {
                "正在发现你的应用…"
            } else {
                "还没有应用入口 · 输入“插件”安装插件"
            }
        } else {
            "从一个应用名称开始。"
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
            color(131, 142, 160),
            DT_SINGLELINE | DT_VCENTER,
        );
    }
    let status = (*s).status.clone();
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
        color(125, 135, 152),
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    let hints = RECT {
        left: client.right - px(s, 328),
        right: client.right - px(s, 18),
        ..footer
    };
    draw_text(
        dc,
        "Alt+数字 / ↑↓ / Ctrl+J/K 选择   ↵ 打开   Esc 返回",
        hints,
        (*s).small_font,
        color(125, 135, 152),
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
    if selected == 1 {
        act(hwnd, Action::Exit);
    }
}

unsafe fn draw_menu_button(hwnd: HWND, draw: &DRAWITEMSTRUCT) {
    let s = state(hwnd);
    let active = matches!((*s).page, Page::Menu);
    FillRect(draw.hDC, &draw.rcItem, (*s).background);
    let brush = CreateSolidBrush(if active || draw.itemState & ODS_SELECTED != 0 {
        color(232, 241, 255)
    } else if (*s).menu_hover {
        color(237, 241, 247)
    } else {
        color(250, 251, 253)
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
    let ink = CreateSolidBrush(color(71, 109, 176));
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
        WM_KEYDOWN if !(*s).composing => {
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
            SetWindowSubclass((*s).edit, Some(edit_proc), 1, hwnd as usize);
            SetWindowSubclass((*s).list, Some(list_proc), 1, hwnd as usize);
            SetWindowSubclass((*s).button, Some(button_proc), 1, hwnd as usize);
            SendMessageW((*s).edit, EM_SETLIMITTEXT, 256, 0);
            SendMessageW(
                (*s).edit,
                EM_SETCUEBANNER,
                1,
                wide("搜索应用、拼音或首字母").as_ptr() as isize,
            );
            fonts(hwnd);
            return 0;
        }
        WM_COMMAND => {
            let id = w & 0xffff;
            let event = (w >> 16) & 0xffff;
            if id == EDIT_ID && event == EN_CHANGE as usize && !(*s).suppress_edit {
                let mut text = vec![0u16; 258];
                let len = GetWindowTextW((*s).edit, text.as_mut_ptr(), text.len() as i32);
                (*s).query = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                rebuild(hwnd);
            } else if id == LIST_ID && event == LBN_DBLCLK as usize {
                execute(hwnd);
            } else if id == LIST_ID && event == LBN_SELCHANGE as usize {
                SetFocus((*s).edit);
            } else if id == MENU_ID && event == BN_CLICKED as usize {
                menu(hwnd);
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
                        navigate(hwnd, Page::Home);
                        hide(hwnd, true);
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
                VK_TAB => menu(hwnd),
                _ => {}
            }
            return 0;
        }
        WM_HOTKEY => {
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
            SetBkColor(w as HDC, color(250, 251, 253));
            SetTextColor(w as HDC, color(30, 39, 56));
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
            UnregisterHotKey(hwnd, HOTKEY_ID);
            Shell_NotifyIconW(NIM_DELETE, &(*s).tray);
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}
