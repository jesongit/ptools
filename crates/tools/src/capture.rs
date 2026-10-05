use crate::{
    history::History,
    interactive::{Invocation, WM_INVOKE, read_invocations},
    native::*,
    ocr::{Recognition, TextSelection},
};
use image::{AnimationDecoder, Rgba, RgbaImage};
use ptools_core::{Result, read_json, write_json};
use ptools_ui as theme;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::BufReader,
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const WM_IDLE: u32 = WM_APP + 21;
const WM_OCR: u32 = WM_APP + 22;
const WM_COMMIT_TEXT: u32 = WM_APP + 23;
mod long_panel;
mod text_editor;
mod toolbar;

const LONG_HOTKEYS: [(i32, u32, u32, usize); 5] = [
    (301, MOD_NOREPEAT, VK_RETURN as u32, 201),
    (302, MOD_NOREPEAT, VK_ESCAPE as u32, 205),
    (303, MOD_NOREPEAT | MOD_CONTROL, 0x43, 201),
    (304, MOD_NOREPEAT | MOD_CONTROL, 0x54, 202),
    (305, MOD_NOREPEAT | MOD_CONTROL, 0x53, 203),
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub history_days: u32,
    pub color_format: String,
    pub detect_ui: bool,
    pub include_cursor: bool,
    pub radius: u32,
    pub shadow: bool,
    pub border: bool,
    pub fixed_ratio: Option<[u32; 2]>,
    pub delay_ms: u32,
    pub region: Option<[i32; 4]>,
    pub presets: Vec<[i32; 4]>,
    pub translation: crate::translation::Settings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            history_days: 7,
            color_format: "HEX".into(),
            detect_ui: true,
            include_cursor: false,
            radius: 0,
            shadow: false,
            border: false,
            fixed_ratio: None,
            delay_ms: 0,
            region: None,
            presets: vec![],
            translation: Default::default(),
        }
    }
}

struct App {
    hwnd: HWND,
    root: PathBuf,
    settings: Settings,
    windows: HashSet<usize>,
    busy: bool,
    delayed: bool,
}
type Frames = Box<dyn Iterator<Item = image::ImageResult<image::Frame>>>;
struct Animation {
    path: PathBuf,
    frames: Frames,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Select,
    PickColor,
    Line,
    Polyline,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Highlight,
    Text,
    Number,
}
#[derive(Clone)]
struct Mark {
    tool: Tool,
    points: Vec<[i32; 2]>,
    text: String,
    color: u32,
    width: i32,
}
struct View {
    app: *mut App,
    owned: bool,
    token: usize,
    working: bool,
    image: RgbaImage,
    screen: Option<RECT>,
    selection: RECT,
    hover: RECT,
    tool: Tool,
    dragging: bool,
    quick: bool,
    start: [i32; 2],
    marks: Vec<Mark>,
    redo: Vec<Mark>,
    point: [i32; 2],
    detect_at: Instant,
    font: HFONT,
    alpha: u8,
    recognition: Option<Recognition>,
    recognition_origin: [i32; 2],
    recognition_generation: u64,
    recognition_pending: bool,
    recognition_attempted: bool,
    recognition_error: Option<String>,
    text_selection_enabled: bool,
    text_selection: TextSelection,
    selecting_text: bool,
    polyline_active: bool,
    animation: Option<Animation>,
    file: Option<PathBuf>,
    long: Option<LongCapture>,
    history: Vec<crate::history::HistoryItem>,
    list: HWND,
    color: u32,
    stroke: i32,
    editing: bool,
    toolbar_window: HWND,
    hover_button: Option<usize>,
    group_tools: [usize; 3],
    selection_drag: u8,
    selection_origin: RECT,
    text_editor: Option<text_editor::Editor>,
}
struct OcrWork {
    window: usize,
    token: usize,
    generation: u64,
    origin: [i32; 2],
    translate: bool,
    result: Result<(Recognition, Option<String>)>,
}
struct LongCapture {
    region: RECT,
    previous: RgbaImage,
    image: RgbaImage,
    paused: bool,
    status: String,
    hotkeys: Vec<i32>,
}

unsafe fn app(hwnd: HWND) -> *mut App {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App
}
unsafe fn view(hwnd: HWND) -> *mut View {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut View
}
unsafe fn image_point(hwnd: HWND, data: &View, p: [i32; 2]) -> [i32; 2] {
    if data.screen.is_some() {
        return p;
    }
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    [
        ((p[0] - 2) as i64 * data.image.width() as i64 / (client.right - 4).max(1) as i64) as i32,
        ((p[1] - 2) as i64 * data.image.height() as i64 / (client.bottom - 4).max(1) as i64) as i32,
    ]
}
fn point(l: LPARAM) -> [i32; 2] {
    [(l as u16 as i16) as i32, ((l >> 16) as u16 as i16) as i32]
}
fn rect(a: [i32; 2], b: [i32; 2]) -> RECT {
    RECT {
        left: a[0].min(b[0]),
        top: a[1].min(b[1]),
        right: a[0].max(b[0]),
        bottom: a[1].max(b[1]),
    }
}
fn contains(r: RECT, p: [i32; 2]) -> bool {
    p[0] >= r.left && p[0] < r.right && p[1] >= r.top && p[1] < r.bottom
}
fn crop(image: &RgbaImage, r: RECT) -> Result<RgbaImage> {
    let left = r.left.clamp(0, image.width() as i32);
    let top = r.top.clamp(0, image.height() as i32);
    let right = r.right.clamp(left, image.width() as i32);
    let bottom = r.bottom.clamp(top, image.height() as i32);
    if left == right || top == bottom {
        return Err("请先选择截图区域".into());
    }
    Ok(image::imageops::crop_imm(
        image,
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    )
    .to_image())
}
fn selection_hit(r: RECT, p: [i32; 2], margin: i32) -> u8 {
    if r.right <= r.left
        || r.bottom <= r.top
        || p[0] < r.left - margin
        || p[0] > r.right + margin
        || p[1] < r.top - margin
        || p[1] > r.bottom + margin
    {
        return 0;
    }
    let mut edges = 0;
    if (p[0] - r.left).abs() <= margin {
        edges |= 1;
    } else if (p[0] - r.right).abs() <= margin {
        edges |= 2;
    }
    if (p[1] - r.top).abs() <= margin {
        edges |= 4;
    } else if (p[1] - r.bottom).abs() <= margin {
        edges |= 8;
    }
    if edges == 0 { 16 } else { edges }
}
fn adjust_selection(data: &mut View, p: [i32; 2]) {
    let origin = data.selection_origin;
    let dx = p[0] - data.start[0];
    let dy = p[1] - data.start[1];
    let width = data.image.width() as i32;
    let height = data.image.height() as i32;
    let mut r = origin;
    if data.selection_drag == 16 {
        let dx = dx.clamp(-origin.left, width - origin.right);
        let dy = dy.clamp(-origin.top, height - origin.bottom);
        r = RECT {
            left: origin.left + dx,
            top: origin.top + dy,
            right: origin.right + dx,
            bottom: origin.bottom + dy,
        };
        let moved = [r.left - data.selection.left, r.top - data.selection.top];
        for mark in data.marks.iter_mut().chain(data.redo.iter_mut()) {
            for point in &mut mark.points {
                point[0] += moved[0];
                point[1] += moved[1];
            }
        }
    } else {
        if data.selection_drag & 1 != 0 {
            r.left = (origin.left + dx).clamp(0, origin.right - 1);
        }
        if data.selection_drag & 2 != 0 {
            r.right = (origin.right + dx).clamp(origin.left + 1, width);
        }
        if data.selection_drag & 4 != 0 {
            r.top = (origin.top + dy).clamp(0, origin.bottom - 1);
        }
        if data.selection_drag & 8 != 0 {
            r.bottom = (origin.bottom + dy).clamp(origin.top + 1, height);
        }
    }
    data.selection = r;
}
unsafe fn create_view(
    state: &mut App,
    mut data: Box<View>,
    title: &str,
    r: RECT,
    overlay: bool,
) -> HWND {
    let class = wide("ptools.capture.view");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        lpfnWndProc: Some(view_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), if overlay { IDC_CROSS } else { IDC_ARROW }),
        style: CS_DBLCLKS,
        ..zeroed()
    };
    RegisterClassW(&wc);
    data.app = state;
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
        class.as_ptr(),
        wide(title).as_ptr(),
        WS_POPUP | WS_CLIPCHILDREN,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        null_mut(),
        null_mut(),
        module,
        (&mut *data as *mut View).cast(),
    );
    if !hwnd.is_null() {
        data.owned = true;
        let _ = Box::into_raw(data);
        state.windows.insert(hwnd as usize);
        SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    hwnd
}
fn empty_view(image: RgbaImage) -> Box<View> {
    Box::new(View {
        app: null_mut(),
        owned: false,
        token: {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        },
        working: false,
        image,
        screen: None,
        selection: RECT::default(),
        hover: RECT::default(),
        tool: Tool::Select,
        dragging: false,
        quick: false,
        start: [0, 0],
        marks: vec![],
        redo: vec![],
        point: [0, 0],
        detect_at: Instant::now(),
        font: null_mut(),
        alpha: 255,
        recognition: None,
        recognition_origin: [0, 0],
        recognition_generation: 0,
        recognition_pending: false,
        recognition_attempted: false,
        recognition_error: None,
        text_selection_enabled: true,
        text_selection: TextSelection::default(),
        selecting_text: false,
        polyline_active: false,
        animation: None,
        file: None,
        long: None,
        history: vec![],
        list: null_mut(),
        color: toolbar::COLORS[0],
        stroke: 3,
        editing: false,
        toolbar_window: null_mut(),
        hover_button: None,
        group_tools: [1, 3, 4],
        selection_drag: 0,
        selection_origin: RECT::default(),
        text_editor: None,
    })
}
unsafe fn store(state: &App, image: &RgbaImage, source: &str) -> Result<()> {
    History::new(&state.root, state.settings.history_days)?
        .add(image, source)
        .map(|_| ())
}
unsafe fn pin(
    state: &mut App,
    image: RgbaImage,
    animate: Option<PathBuf>,
    remember: bool,
) -> Result<HWND> {
    if remember {
        store(state, &image, "贴图")?;
    }
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor);
    let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    GetMonitorInfoW(monitor, &mut info);
    let scale = (0.8 * (info.rcWork.right - info.rcWork.left) as f32 / image.width() as f32)
        .min(0.8 * (info.rcWork.bottom - info.rcWork.top) as f32 / image.height() as f32)
        .min(1.0);
    let width = (image.width() as f32 * scale).max(32.0) as i32 + 4;
    let height = (image.height() as f32 * scale).max(32.0) as i32 + 4;
    let r = RECT {
        left: info.rcWork.left + ((info.rcWork.right - info.rcWork.left) - width) / 2,
        top: info.rcWork.top + ((info.rcWork.bottom - info.rcWork.top) - height) / 2,
        right: 0,
        bottom: 0,
    };
    let mut data = empty_view(image);
    data.file = animate.clone();
    data.animation =
        animate.and_then(|path| frames(&path).ok().map(|frames| Animation { path, frames }));
    let hwnd = create_view(
        state,
        data,
        "ptools 贴图 · 空格标注 · 右键菜单 · Esc关闭",
        RECT {
            right: r.left + width,
            bottom: r.top + height,
            ..r
        },
        false,
    );
    if hwnd.is_null() {
        return Err("无法打开贴图窗口".into());
    }
    if (*view(hwnd)).animation.is_some() {
        SetTimer(hwnd, 2, 100, None);
    }
    Ok(hwnd)
}
fn frames(path: &Path) -> Result<Frames> {
    let file = BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?);
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "gif" => Ok(Box::new(
            image::codecs::gif::GifDecoder::new(file)
                .map_err(|e| e.to_string())?
                .into_frames(),
        )),
        "webp" => Ok(Box::new(
            image::codecs::webp::WebPDecoder::new(file)
                .map_err(|e| e.to_string())?
                .into_frames(),
        )),
        _ => Err("不是动图".into()),
    }
}
unsafe fn clipboard_image(hwnd: HWND) -> Result<(RgbaImage, Option<PathBuf>)> {
    if OpenClipboard(hwnd) == 0 {
        return Err("剪贴板正被其他程序占用".into());
    }
    let result = (|| {
        let drop = GetClipboardData(15);
        if !drop.is_null() {
            let mut path = vec![0u16; 32768];
            let len = DragQueryFileW(drop, 0, path.as_mut_ptr(), path.len() as u32);
            let path = PathBuf::from(String::from_utf16_lossy(&path[..len as usize]));
            if let Ok(image) = image::open(&path) {
                return Ok((image.to_rgba8(), Some(path)));
            }
            return text_image(&format!(
                "{}\n{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                path.display()
            ))
            .map(|i| (i, Some(path)));
        }
        let bitmap = GetClipboardData(2);
        if !bitmap.is_null() {
            let mut object: BITMAP = zeroed();
            GetObjectW(
                bitmap,
                size_of::<BITMAP>() as i32,
                (&mut object as *mut BITMAP).cast(),
            );
            let width = object.bmWidth;
            let height = object.bmHeight.abs();
            if width > 0 && height > 0 && width as u64 * height as u64 <= 100_000_000 {
                let mut info: BITMAPINFO = zeroed();
                info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = width;
                info.bmiHeader.biHeight = -height;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                let mut pixels = vec![0u8; width as usize * height as usize * 4];
                let dc = GetDC(null_mut());
                let read = GetDIBits(
                    dc,
                    bitmap,
                    0,
                    height as u32,
                    pixels.as_mut_ptr().cast(),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                ReleaseDC(null_mut(), dc);
                if read != 0 {
                    for p in pixels.as_chunks_mut::<4>().0 {
                        p.swap(0, 2);
                        p[3] = 255;
                    }
                    return Ok((
                        RgbaImage::from_raw(width as u32, height as u32, pixels).unwrap(),
                        None,
                    ));
                }
            }
        }
        Err("剪贴板没有可用图片".into())
    })();
    CloseClipboard();
    if result.is_ok() {
        return result;
    }
    let text = clipboard_text(hwnd)
        .filter(|s| !s.is_empty())
        .ok_or("剪贴板中没有图片、文字或文件")?;
    let image = if let Some(color) = parse_color(&text) {
        RgbaImage::from_pixel(260, 160, color)
    } else {
        text_image(&text)?
    };
    Ok((image, None))
}
unsafe fn quick_pin(state: &mut App, image: RgbaImage, region: RECT) -> Result<()> {
    store(state, &image, "截图")?;
    let copied = copy_image(state.hwnd, &image);
    let width = image.width() as i32;
    let height = image.height() as i32;
    let window = pin(state, image, None, false)?;
    SetWindowPos(
        window,
        HWND_TOPMOST,
        region.left - 2,
        region.top - 2,
        width + 4,
        height + 4,
        SWP_NOACTIVATE,
    );
    copied
}

unsafe fn quick_region(state: &mut App, region: [i32; 4]) -> Result<()> {
    let [x, y, width, height] = region;
    let right = x.checked_add(width).ok_or("截图区域超出范围")?;
    let bottom = y.checked_add(height).ok_or("截图区域超出范围")?;
    let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let screen_right = screen_x + GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let screen_bottom = screen_y + GetSystemMetrics(SM_CYVIRTUALSCREEN);
    let region = RECT {
        left: x.clamp(screen_x, screen_right),
        top: y.clamp(screen_y, screen_bottom),
        right: right.clamp(screen_x, screen_right),
        bottom: bottom.clamp(screen_y, screen_bottom),
    };
    if width < 4 || height < 4 || region.right - region.left < 4 || region.bottom - region.top < 4 {
        return Err("截图区域太小或不在屏幕内".into());
    }
    let mut image = screenshot(region)?;
    if state.settings.include_cursor {
        draw_cursor(&mut image, region);
    }
    let mut data = empty_view(image);
    data.app = state;
    data.screen = Some(region);
    data.selection = RECT {
        left: 0,
        top: 0,
        right: region.right - region.left,
        bottom: region.bottom - region.top,
    };
    let image = rendered(&data)?;
    quick_pin(state, image, region)
}

unsafe fn capture(state: &mut App, custom: bool, quick: bool) -> Result<()> {
    if custom {
        let saved = state.settings.region.unwrap_or([0, 0, 800, 600]);
        let preset_note = state
            .settings
            .presets
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}: {},{} {}×{}", i + 1, r[0], r[1], r[2], r[3]))
            .collect::<Vec<_>>()
            .join("；");
        let fields = [
            Field::text("延时（毫秒，0–60000）", state.settings.delay_ms),
            Field::check(
                "使用固定区域（不勾选则框选）",
                state.settings.region.is_some(),
            ),
            Field::text("区域左边 X（可为负数）", saved[0]),
            Field::text("区域顶部 Y（可为负数）", saved[1]),
            Field::text("区域宽度（像素）", saved[2]),
            Field::text("区域高度（像素）", saved[3]),
            Field::text("使用已保存区域编号（留空不使用）", ""),
            Field::check("将本次区域存为预设", false),
        ];
        let Some(values) = form(
            state.hwnd,
            "延时与固定区域截图",
            &format!("区域坐标使用屏幕物理像素。预设：{preset_note}"),
            &fields,
        ) else {
            return Ok(());
        };
        let delay = values[0].parse::<u32>().map_err(|_| "延时应为整数")?;
        if delay > 60000 {
            return Err("延时不超过60000毫秒".into());
        }
        state.settings.delay_ms = delay;
        let region = if values[1] == "true" {
            let mut region = [0; 4];
            for i in 0..4 {
                region[i] = values[i + 2]
                    .parse::<i32>()
                    .map_err(|_| "区域坐标和尺寸应为整数")?;
            }
            if region[2] <= 0
                || region[3] <= 0
                || region[0].checked_add(region[2]).is_none()
                || region[1].checked_add(region[3]).is_none()
            {
                return Err("区域尺寸无效".into());
            }
            Some(region)
        } else {
            None
        };
        state.settings.region = if values[6].trim().is_empty() {
            region
        } else {
            let i = values[6].parse::<usize>().map_err(|_| "预设编号无效")?;
            Some(
                *state
                    .settings
                    .presets
                    .get(i.wrapping_sub(1))
                    .ok_or("预设不存在")?,
            )
        };
        if values[7] == "true"
            && let Some(r) = state.settings.region
        {
            if !state.settings.presets.contains(&r) {
                state.settings.presets.push(r);
            }
            state.settings.presets.truncate(20);
        }
        write_json(&state.root.join("settings.json"), &state.settings)?;
        state.delayed = true;
        SetTimer(state.hwnd, 4, delay.max(1), None);
        return Ok(());
    }
    let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let r = RECT {
        left: x,
        top: y,
        right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
        bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
    };
    let mut image = screenshot(r)?;
    if state.settings.include_cursor {
        draw_cursor(&mut image, r);
    }
    let mut data = empty_view(image);
    data.screen = Some(r);
    data.quick = quick;
    let title = if quick {
        "ptools 快速截图 · 松开鼠标贴图"
    } else {
        "ptools 截图"
    };
    let hwnd = create_view(state, data, title, r, true);
    if hwnd.is_null() {
        return Err("无法打开截图界面".into());
    }
    Ok(())
}
unsafe fn draw_cursor(image: &mut RgbaImage, screen: RECT) {
    let mut cursor: CURSORINFO = zeroed();
    cursor.cbSize = size_of::<CURSORINFO>() as u32;
    if GetCursorInfo(&mut cursor) == 0 || cursor.flags & CURSOR_SHOWING == 0 {
        return;
    }
    let mut icon: ICONINFO = zeroed();
    if GetIconInfo(cursor.hCursor, &mut icon) == 0 {
        return;
    }
    paint_native(image, |dc| {
        DrawIconEx(
            dc,
            cursor.ptScreenPos.x - screen.left - icon.xHotspot as i32,
            cursor.ptScreenPos.y - screen.top - icon.yHotspot as i32,
            cursor.hCursor,
            0,
            0,
            0,
            null_mut(),
            DI_NORMAL,
        );
    });
    if !icon.hbmColor.is_null() {
        DeleteObject(icon.hbmColor);
    }
    if !icon.hbmMask.is_null() {
        DeleteObject(icon.hbmMask);
    }
}
unsafe fn paint_native(image: &mut RgbaImage, draw: impl FnOnce(HDC)) {
    let dc = CreateCompatibleDC(null_mut());
    let mut info: BITMAPINFO = zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = image.width() as i32;
    info.bmiHeader.biHeight = -(image.height() as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if !bitmap.is_null() {
        {
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), image.as_raw().len());
            pixels.copy_from_slice(image.as_raw());
            for p in pixels.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
        }
        let old = SelectObject(dc, bitmap);
        draw(dc);
        let len = image.as_raw().len();
        image
            .as_mut()
            .copy_from_slice(std::slice::from_raw_parts(bits.cast::<u8>(), len));
        for p in image.as_mut().as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            p[3] = 255;
        }
        SelectObject(dc, old);
        DeleteObject(bitmap);
    }
    DeleteDC(dc);
}
unsafe fn detect(p: [i32; 2], screen: RECT, detailed: bool) -> RECT {
    struct Hit {
        point: POINT,
        hwnd: HWND,
        bounds: RECT,
    }
    unsafe extern "system" fn find(hwnd: HWND, l: LPARAM) -> i32 {
        let hit = &mut *(l as *mut Hit);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || pid == std::process::id() {
            return 1;
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r);
        if contains(r, [hit.point.x, hit.point.y]) {
            hit.hwnd = hwnd;
            hit.bounds = r;
            return 0;
        }
        1
    }
    let mut hit = Hit {
        point: POINT {
            x: p[0] + screen.left,
            y: p[1] + screen.top,
        },
        hwnd: null_mut(),
        bounds: screen,
    };
    EnumWindows(Some(find), (&mut hit as *mut Hit) as isize);
    let mut r = hit.bounds;
    if detailed && !hit.hwnd.is_null() {
        use windows::Win32::{
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            UI::Accessibility::{CUIAutomation, IUIAutomation},
        };
        if let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            && let Ok(mut element) =
                automation.ElementFromHandle(windows::Win32::Foundation::HWND(hit.hwnd))
            && let Ok(walker) = automation.ControlViewWalker()
        {
            // Walk only the branch containing the pointer, without hiding the capture surface.
            for _ in 0..20 {
                let Ok(mut child) = walker.GetFirstChildElement(&element) else {
                    break;
                };
                let mut found = None;
                for _ in 0..128 {
                    if let Ok(bounds) = child.CurrentBoundingRectangle() {
                        let candidate = RECT {
                            left: bounds.left,
                            top: bounds.top,
                            right: bounds.right,
                            bottom: bounds.bottom,
                        };
                        if contains(candidate, [hit.point.x, hit.point.y])
                            && candidate.right > candidate.left
                            && candidate.bottom > candidate.top
                        {
                            r = candidate;
                            found = Some(child);
                            break;
                        }
                    }
                    match walker.GetNextSiblingElement(&child) {
                        Ok(next) => child = next,
                        Err(_) => break,
                    }
                }
                match found {
                    Some(child) => element = child,
                    None => break,
                }
            }
        }
    }
    RECT {
        left: (r.left - screen.left).clamp(0, screen.right - screen.left),
        top: (r.top - screen.top).clamp(0, screen.bottom - screen.top),
        right: (r.right - screen.left).clamp(0, screen.right - screen.left),
        bottom: (r.bottom - screen.top).clamp(0, screen.bottom - screen.top),
    }
}

unsafe fn rendered(data: &View) -> Result<RgbaImage> {
    render_image(data, true)
}
unsafe fn mosaic_stroke(image: &mut RgbaImage, points: &[[i32; 2]], width: i32) {
    let block = (width * 2).max(6);
    let diameter = (width * 8).max(16);
    let mut mask = RgbaImage::new(image.width(), image.height());
    paint_native(&mut mask, |dc| {
        let pen = CreatePen(PS_SOLID, diameter, 0xffffff);
        let brush = CreateSolidBrush(0xffffff);
        let old_pen = SelectObject(dc, pen);
        let old_brush = SelectObject(dc, brush);
        MoveToEx(dc, points[0][0], points[0][1], null_mut());
        for p in points.iter().skip(1) {
            LineTo(dc, p[0], p[1]);
        }
        // Round caps also make a single click paint a complete brush dab.
        SelectObject(dc, GetStockObject(NULL_PEN));
        for p in points {
            Ellipse(
                dc,
                p[0] - diameter / 2,
                p[1] - diameter / 2,
                p[0] + diameter / 2,
                p[1] + diameter / 2,
            );
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pen);
        DeleteObject(brush);
    });
    let left = (points.iter().map(|p| p[0]).min().unwrap() - diameter).max(0) / block * block;
    let top = (points.iter().map(|p| p[1]).min().unwrap() - diameter).max(0) / block * block;
    let right = (points.iter().map(|p| p[0]).max().unwrap() + diameter).min(image.width() as i32);
    let bottom = (points.iter().map(|p| p[1]).max().unwrap() + diameter).min(image.height() as i32);
    for y in (top..bottom).step_by(block as usize) {
        for x in (left..right).step_by(block as usize) {
            let end_x = (x + block).min(image.width() as i32);
            let end_y = (y + block).min(image.height() as i32);
            let mut sum = [0u32; 3];
            let mut count = 0;
            for yy in y..end_y {
                for xx in x..end_x {
                    let pixel = image.get_pixel(xx as u32, yy as u32);
                    for c in 0..3 {
                        sum[c] += pixel[c] as u32;
                    }
                    count += 1;
                }
            }
            for yy in y..end_y {
                for xx in x..end_x {
                    if mask.get_pixel(xx as u32, yy as u32)[0] != 0 {
                        let pixel = image.get_pixel_mut(xx as u32, yy as u32);
                        for c in 0..3 {
                            pixel[c] = (sum[c] / count) as u8;
                        }
                    }
                }
            }
        }
    }
}
unsafe fn render_image(data: &View, decorate: bool) -> Result<RgbaImage> {
    let mut image = if data.screen.is_some() {
        crop(&data.image, data.selection)?
    } else {
        data.image.clone()
    };
    let origin = if data.screen.is_some() {
        [data.selection.left, data.selection.top]
    } else {
        [0, 0]
    };
    for mark in &data.marks {
        let points: Vec<_> = mark
            .points
            .iter()
            .map(|p| [p[0] - origin[0], p[1] - origin[1]])
            .collect();
        if points.is_empty() {
            continue;
        }
        let a = points[0];
        let b = *points.last().unwrap();
        let r = rect(a, b);
        if mark.tool == Tool::Pen && mark.color == toolbar::MOSAIC_COLOR {
            mosaic_stroke(&mut image, &points, mark.width);
            continue;
        }
        if mark.tool == Tool::Highlight {
            let mut mask = RgbaImage::new(image.width(), image.height());
            paint_native(&mut mask, |dc| {
                // A white coverage mask also supports black highlighter ink.
                let pen = CreatePen(PS_SOLID, mark.width * 4, 0xffffff);
                let old = SelectObject(dc, pen);
                MoveToEx(dc, a[0], a[1], null_mut());
                for p in points.iter().skip(1) {
                    LineTo(dc, p[0], p[1]);
                }
                SelectObject(dc, old);
                DeleteObject(pen);
            });
            for (pixel, coverage) in image.pixels_mut().zip(mask.pixels()) {
                if coverage[0] > 0 || coverage[1] > 0 || coverage[2] > 0 {
                    for c in 0..3 {
                        let ink = ((mark.color >> (c * 8)) & 255) as u16;
                        pixel[c] = ((pixel[c] as u16 * 3 + ink * 2) / 5) as u8;
                    }
                }
            }
            continue;
        }
        let image_width = image.width() as i32;
        let image_height = image.height() as i32;
        paint_native(&mut image, |dc| {
            let pen = CreatePen(PS_SOLID, mark.width, mark.color);
            let old_pen = SelectObject(dc, pen);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            match mark.tool {
                Tool::Rect => {
                    Rectangle(dc, r.left, r.top, r.right, r.bottom);
                }
                Tool::Ellipse => {
                    Ellipse(dc, r.left, r.top, r.right, r.bottom);
                }
                Tool::Line | Tool::Arrow => {
                    MoveToEx(dc, a[0], a[1], null_mut());
                    LineTo(dc, b[0], b[1]);
                    let angle = ((b[1] - a[1]) as f32).atan2((b[0] - a[0]) as f32);
                    for delta in if mark.tool == Tool::Arrow {
                        &[-0.5f32, 0.5][..]
                    } else {
                        &[][..]
                    } {
                        MoveToEx(dc, b[0], b[1], null_mut());
                        LineTo(
                            dc,
                            b[0] - (18.0 * (angle + delta).cos()) as i32,
                            b[1] - (18.0 * (angle + delta).sin()) as i32,
                        );
                    }
                }
                Tool::Pen | Tool::Highlight | Tool::Polyline => {
                    MoveToEx(dc, a[0], a[1], null_mut());
                    for p in points.iter().skip(1) {
                        LineTo(dc, p[0], p[1]);
                    }
                }
                Tool::Text | Tool::Number => {
                    let size = 18 + mark.width * 2;
                    let font = CreateFontW(
                        -size,
                        0,
                        0,
                        0,
                        FW_BOLD as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET as u32,
                        0,
                        0,
                        0,
                        0,
                        wide("Microsoft YaHei").as_ptr(),
                    );
                    let old = SelectObject(dc, font);
                    SetBkMode(dc, TRANSPARENT as i32);
                    SetTextColor(dc, mark.color);
                    let mut r = RECT {
                        left: a[0],
                        top: a[1],
                        right: 4096,
                        bottom: image_height,
                    };
                    if mark.tool == Tool::Number {
                        let radius = size * (mark.text.len() as i32 + 1) / 3 + 5;
                        let brush = CreateSolidBrush(mark.color);
                        let previous = SelectObject(dc, brush);
                        Ellipse(
                            dc,
                            a[0] - radius,
                            a[1] - radius,
                            a[0] + radius,
                            a[1] + radius,
                        );
                        SelectObject(dc, previous);
                        DeleteObject(brush);
                        let luminance = (mark.color & 255) * 299
                            + ((mark.color >> 8) & 255) * 587
                            + ((mark.color >> 16) & 255) * 114;
                        SetTextColor(
                            dc,
                            if luminance > 150_000 {
                                0x252525
                            } else {
                                0xffffff
                            },
                        );
                        r = RECT {
                            left: a[0] - radius,
                            top: a[1] - radius,
                            right: a[0] + radius,
                            bottom: a[1] + radius,
                        };
                        DrawTextW(
                            dc,
                            wide(&mark.text).as_ptr(),
                            -1,
                            &mut r,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                        );
                    } else {
                        r.right = image_width;
                        DrawTextW(
                            dc,
                            wide(&mark.text).as_ptr(),
                            -1,
                            &mut r,
                            DT_WORDBREAK | DT_NOPREFIX,
                        );
                    }
                    SelectObject(dc, old);
                    DeleteObject(font);
                }
                _ => {}
            }
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            DeleteObject(pen);
        });
    }
    if decorate && data.screen.is_some() {
        let settings = &(*data.app).settings;
        let radius = settings
            .radius
            .min(image.width() / 2)
            .min(image.height() / 2) as i32;
        if radius > 0 {
            let w = image.width() as i32;
            let h = image.height() as i32;
            for y in 0..h {
                for x in 0..w {
                    let dx = (radius - x).max(x - (w - 1 - radius)).max(0);
                    let dy = (radius - y).max(y - (h - 1 - radius)).max(0);
                    if dx * dx + dy * dy > radius * radius {
                        image.get_pixel_mut(x as u32, y as u32)[3] = 0;
                    }
                }
            }
        }
        if settings.border {
            for x in 0..image.width() {
                let h = image.height();
                image.put_pixel(x, 0, Rgba([80, 80, 80, 255]));
                image.put_pixel(x, h - 1, Rgba([80, 80, 80, 255]));
            }
            for y in 0..image.height() {
                let w = image.width();
                image.put_pixel(0, y, Rgba([80, 80, 80, 255]));
                image.put_pixel(w - 1, y, Rgba([80, 80, 80, 255]));
            }
        }
        if settings.shadow {
            let mut out = RgbaImage::new(image.width() + 24, image.height() + 24);
            for y in 0..image.height() + 16 {
                for x in 0..image.width() + 16 {
                    let edge = x
                        .min(y)
                        .min(image.width() + 15 - x)
                        .min(image.height() + 15 - y)
                        .min(8);
                    out.put_pixel(x + 4, y + 4, Rgba([0, 0, 0, (edge * 10) as u8]));
                }
            }
            image::imageops::overlay(&mut out, &image, 8, 8);
            image = out;
        }
    }
    Ok(image)
}

fn selected_tool(id: usize) -> Option<Tool> {
    match id {
        0 => Some(Tool::Select),
        1 => Some(Tool::Rect),
        2 => Some(Tool::Ellipse),
        3 => Some(Tool::Arrow),
        4 => Some(Tool::Pen),
        5 => Some(Tool::Highlight),
        6 => Some(Tool::Text),
        7 => Some(Tool::Number),
        24 => Some(Tool::PickColor),
        17 => Some(Tool::Line),
        18 => Some(Tool::Polyline),
        _ => None,
    }
}
fn finish_polyline(data: &mut View) {
    if data.polyline_active {
        if let Some(mark) = data.marks.last_mut() {
            mark.points.pop(); // The last point is only the moving preview.
            if mark.points.len() < 2 {
                data.marks.pop();
            }
        }
        data.polyline_active = false;
    }
}
unsafe fn run_ocr(hwnd: HWND, data: &mut View, translate: bool) -> Result<()> {
    if data.recognition_pending {
        return Err("正在识别，请等待完成".into());
    }
    let image = render_image(data, false)?;
    let origin = if data.screen.is_some() {
        [data.selection.left, data.selection.top]
    } else {
        [0, 0]
    };
    let root = (*data.app).root.clone();
    fs::create_dir_all(root.join("work")).map_err(|e| e.to_string())?;
    let file = root.join("work").join(format!(
        "ocr-{}-{}-{}.png",
        std::process::id(),
        data.token,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    image.save(&file).map_err(|e| e.to_string())?;
    let settings = (*data.app).settings.translation.clone();
    let controller = (*data.app).hwnd as usize;
    let window = hwnd as usize;
    let token = data.token;
    let generation = data.recognition_generation;
    data.recognition_pending = true;
    data.recognition_attempted = true;
    data.recognition_error = None;
    data.working = translate;
    std::thread::spawn(move || {
        use std::{
            io::Read,
            os::windows::process::CommandExt,
            process::{Command, Stdio},
        };
        let result = (|| -> Result<(Recognition, Option<String>)> {
            let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .arg("--ocr")
                .arg(&file)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            let _job = match ptools_core::PluginJob::attach(&child) {
                Ok(job) => job,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e);
                }
            };
            let mut stdout = child.stdout.take().unwrap();
            let mut stderr = child.stderr.take().unwrap();
            let output = std::thread::spawn(move || {
                let mut bytes = vec![];
                stdout
                    .by_ref()
                    .take(8 * 1024 * 1024)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes)
            });
            let errors = std::thread::spawn(move || {
                let mut bytes = vec![];
                let _ = stderr.by_ref().take(65536).read_to_end(&mut bytes);
                bytes
            });
            let start = Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status;
                }
                if start.elapsed().as_secs() >= 30 {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("离线识别超时，请缩小图片后重试".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            };
            let bytes = output
                .join()
                .map_err(|_| "无法读取识别结果")?
                .map_err(|e| e.to_string())?;
            let errors = errors.join().map_err(|_| "无法读取识别错误")?;
            if !status.success() {
                return Err(String::from_utf8_lossy(&errors).into_owned());
            }
            let recognition: Recognition =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if translate && recognition.text.trim().is_empty() {
                return Err("没有识别到文字，请选择更清晰的区域".into());
            }
            let translated = if translate {
                Some(crate::translation::translate(&settings, &recognition.text)?)
            } else {
                None
            };
            Ok((recognition, translated))
        })();
        let _ = fs::remove_file(&file);
        let ptr = Box::into_raw(Box::new(OcrWork {
            window,
            token,
            generation,
            origin,
            translate,
            result,
        }));
        unsafe {
            if PostMessageW(controller as HWND, WM_OCR, 0, ptr as isize) == 0 {
                drop(Box::from_raw(ptr));
            }
        }
    });
    Ok(())
}

fn clear_recognition(data: &mut View) {
    data.recognition_generation = data.recognition_generation.wrapping_add(1);
    data.recognition = None;
    data.recognition_attempted = false;
    data.recognition_error = None;
    data.text_selection = TextSelection::default();
    data.selecting_text = false;
}

unsafe fn text_hit(hwnd: HWND, data: &View, p: [i32; 2]) -> bool {
    if !data.text_selection_enabled
        || data.recognition_pending
        || data.polyline_active
        || !data.list.is_null()
        || data.long.is_some()
        || matches!(data.tool, Tool::Text | Tool::Number | Tool::PickColor)
        || data.screen.is_some()
            && (!contains(data.selection, p)
                || selection_hit(
                    data.selection,
                    p,
                    (6 * GetDpiForWindow(hwnd).max(96) / 96) as i32,
                ) != 16)
    {
        return false;
    }
    let p = image_point(hwnd, data, p);
    data.recognition.as_ref().is_some_and(|r| {
        r.hit_test([
            (p[0] - data.recognition_origin[0]) as f32,
            (p[1] - data.recognition_origin[1]) as f32,
        ])
    })
}

unsafe fn select_text(hwnd: HWND, data: &mut View, p: [i32; 2]) {
    if let Some(recognition) = &data.recognition {
        let a = image_point(hwnd, data, data.start);
        let b = image_point(hwnd, data, p);
        data.text_selection = recognition.select([
            (a[0] - data.recognition_origin[0]) as f32,
            (a[1] - data.recognition_origin[1]) as f32,
            (b[0] - a[0]) as f32,
            (b[1] - a[1]) as f32,
        ]);
    }
}

unsafe fn copy_selected_text(hwnd: HWND, data: &View) -> Result<()> {
    if !data.text_selection.text.is_empty() {
        copy_text(hwnd, &data.text_selection.text)?;
    }
    Ok(())
}

unsafe fn copy_picked_color(hwnd: HWND, data: &View) -> Result<()> {
    let p = image_point(hwnd, data, data.point);
    if p[0] >= 0
        && p[1] >= 0
        && p[0] < data.image.width() as i32
        && p[1] < data.image.height() as i32
    {
        let pixel = data.image.get_pixel(p[0] as u32, p[1] as u32);
        copy_text(
            hwnd,
            &color_value(*pixel, &(*data.app).settings.color_format),
        )?;
    }
    Ok(())
}

fn append_long_frame(long: &mut LongCapture, next: RgbaImage) {
    if long.paused {
        return;
    }
    let Some(shift) = vertical_shift(&long.previous, &next) else {
        long.status = "画面未能拼接，请慢速滚动".into();
        return;
    };
    if shift > 0 {
        let height = long.image.height().saturating_add(shift);
        if height as u64 * long.image.width() as u64 > 100_000_000 {
            long.paused = true;
            long.status = "已达到图片长度上限".into();
            return;
        }
        let extra = image::imageops::crop_imm(&next, 0, next.height() - shift, next.width(), shift);
        let mut combined = RgbaImage::new(long.image.width(), height);
        image::imageops::overlay(&mut combined, &long.image, 0, 0);
        image::imageops::overlay(
            &mut combined,
            &extra.to_image(),
            0,
            long.image.height() as i64,
        );
        long.image = combined;
    }
    long.previous = next;
    long.status = if long.hotkeys.len() == LONG_HOTKEYS.len() {
        "滚动页面，点击图标完成"
    } else {
        "快捷键冲突，请用图标"
    }
    .into();
}

unsafe fn sample_long(hwnd: HWND) {
    let data = &mut *view(hwnd);
    if data.working {
        return;
    }
    let Some(long) = &data.long else {
        return;
    };
    if long.paused {
        return;
    }
    let region = long.region;
    let panel = data.toolbar_window;
    let hidden = !panel.is_null() && long_panel::overlaps_capture(hwnd, region);
    long_panel::dismiss_tooltip(hwnd);
    if hidden {
        ShowWindow(panel, SW_HIDE);
        windows_sys::Win32::Graphics::Dwm::DwmFlush();
    }
    let result = screenshot(region);
    if hidden {
        ShowWindow(panel, SW_SHOWNA);
    }
    let data = &mut *view(hwnd);
    if let Some(long) = &mut data.long {
        match result {
            Ok(next) => append_long_frame(long, next),
            Err(_) => long.status = "暂时无法采样，请重试".into(),
        }
    }
    long_panel::sync(hwnd);
}

unsafe fn draw_long_frame(dc: HDC, client: RECT, selection: RECT) {
    FillRect(dc, &client, GetStockObject(BLACK_BRUSH) as HBRUSH);
    let brush = CreateSolidBrush(theme::ACCENT);
    // Every border pixel lies outside the half-open capture rectangle.
    for edge in [
        RECT {
            left: selection.left - 2,
            top: selection.top - 2,
            right: selection.left,
            bottom: selection.bottom + 2,
        },
        RECT {
            left: selection.right,
            top: selection.top - 2,
            right: selection.right + 2,
            bottom: selection.bottom + 2,
        },
        RECT {
            left: selection.left,
            top: selection.top - 2,
            right: selection.right,
            bottom: selection.top,
        },
        RECT {
            left: selection.left,
            top: selection.bottom,
            right: selection.right,
            bottom: selection.bottom + 2,
        },
    ] {
        FillRect(dc, &edge, brush);
    }
    DeleteObject(brush);
}

unsafe fn start_long(hwnd: HWND) -> Result<()> {
    let data = &mut *view(hwnd);
    let Some(screen) = data.screen else {
        return Ok(());
    };
    if (*data.app)
        .windows
        .iter()
        .any(|&window| window != hwnd as usize && (*view(window as HWND)).long.is_some())
    {
        return Err("请先完成当前长截图".into());
    }
    let selection = data.selection;
    let region = RECT {
        left: screen.left + selection.left,
        top: screen.top + selection.top,
        right: screen.left + selection.right,
        bottom: screen.top + selection.bottom,
    };
    let previous = crop(&data.image, selection)?;
    let image = render_image(data, false)?;
    clear_recognition(data);
    data.tool = Tool::Select;
    data.hover_button = None;
    data.long = Some(LongCapture {
        region,
        previous,
        image,
        paused: false,
        status: "滚动页面，点击图标完成".into(),
        hotkeys: Vec::new(),
    });
    ShowWindow(hwnd, SW_HIDE);
    windows_sys::Win32::Graphics::Dwm::DwmFlush();
    let target = GetAncestor(
        WindowFromPoint(POINT {
            x: (region.left + region.right) / 2,
            y: (region.top + region.bottom) / 2,
        }),
        GA_ROOT,
    );
    SetWindowLongPtrW(
        hwnd,
        GWL_EXSTYLE,
        GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) as isize,
    );
    SetLayeredWindowAttributes(hwnd, 0, 255, LWA_COLORKEY | LWA_ALPHA);
    SetWindowTextW(
        hwnd,
        wide("ptools 长截图 · 滚动页面 · Enter复制 · Esc取消").as_ptr(),
    );
    InvalidateRect(hwnd, null(), 0);
    ShowWindow(hwnd, SW_SHOWNA);
    UpdateWindow(hwnd);
    long_panel::sync(hwnd);
    if data.toolbar_window.is_null() {
        DestroyWindow(hwnd);
        return Err("无法创建长截图操作栏".into());
    }
    register_long_keys(hwnd);
    SetTimer(hwnd, 3, 500, None);
    if !target.is_null() {
        SetForegroundWindow(target);
    }
    raise_long(hwnd);
    Ok(())
}

unsafe fn raise_long(hwnd: HWND) {
    // Activating an existing topmost target can move it above our frame.
    // Restore the capture UI order while keeping keyboard focus on the page.
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
    long_panel::sync(hwnd);
}

unsafe fn register_long_keys(hwnd: HWND) {
    let data = &mut *view(hwnd);
    for &(id, modifiers, key, _) in &LONG_HOTKEYS {
        if RegisterHotKey(hwnd, id, modifiers, key) != 0 {
            data.long.as_mut().unwrap().hotkeys.push(id);
        }
    }
    if data.long.as_ref().unwrap().hotkeys.len() != LONG_HOTKEYS.len() {
        data.long.as_mut().unwrap().status = "快捷键冲突，请用图标".into();
        long_panel::sync(hwnd);
    }
}

unsafe fn unregister_long_keys(hwnd: HWND) {
    if let Some(long) = &mut (*view(hwnd)).long {
        for id in long.hotkeys.drain(..) {
            UnregisterHotKey(hwnd, id);
        }
    }
}

unsafe fn finish_long(hwnd: HWND, id: usize) -> Result<()> {
    if (*view(hwnd)).working {
        return Ok(());
    }
    if id == 205 || id == 100 {
        DestroyWindow(hwnd);
        return Ok(());
    }
    if id == 204 {
        let data = &mut *view(hwnd);
        if let Some(long) = &mut data.long {
            long.paused = !long.paused;
            long.status = if long.paused {
                "已暂停"
            } else {
                "滚动页面，点击图标完成"
            }
            .into();
        }
        long_panel::sync(hwnd);
        return Ok(());
    }
    if !(201..=203).contains(&id) {
        return Ok(());
    }
    sample_long(hwnd);
    KillTimer(hwnd, 3);
    let foreground = GetForegroundWindow();
    unregister_long_keys(hwnd);
    let data = &mut *view(hwnd);
    data.working = true;
    EnableWindow(data.toolbar_window, 0);
    let image = data.long.as_ref().unwrap().image.clone();
    let result = (|| -> Result<bool> {
        match id {
            203 => {
                let Some(path) = save_dialog(hwnd) else {
                    return Ok(false);
                };
                save_image(&path, &image)?;
                store(&*data.app, &image, "长截图")?;
            }
            202 => {
                store(&*data.app, &image, "长截图")?;
                pin(&mut *data.app, image.clone(), None, false)?;
            }
            _ => {
                store(&*data.app, &image, "长截图")?;
                copy_image(hwnd, &image)?;
            }
        }
        Ok(true)
    })();
    if result.as_ref().is_ok_and(|finished| *finished) {
        DestroyWindow(hwnd);
    } else {
        if let Err(error) = &result {
            alert(hwnd, error);
        }
        if IsWindow(hwnd) == 0 {
            return Ok(());
        }
        let data = &mut *view(hwnd);
        data.working = false;
        EnableWindow(data.toolbar_window, 1);
        register_long_keys(hwnd);
        SetTimer(hwnd, 3, 500, None);
        if IsWindow(foreground) != 0 {
            SetForegroundWindow(foreground);
        }
        raise_long(hwnd);
    }
    Ok(())
}

unsafe fn command(hwnd: HWND, id: usize) -> Result<()> {
    let data = &mut *view(hwnd);
    if data.long.is_some() {
        return finish_long(
            hwnd,
            match id {
                9 => 201,
                10 => 202,
                11 => 203,
                _ => id,
            },
        );
    }
    if !toolbar::enabled(data, id) {
        return Ok(());
    }
    text_editor::finish(hwnd, data, true);
    toolbar::remember_tool(data, id);
    if data.dragging {
        data.dragging = false;
        data.selecting_text = false;
        ReleaseCapture();
    }
    if let Some(tool) = selected_tool(id) {
        finish_polyline(data);
        data.tool = tool;
        if tool != Tool::Pen && data.color == toolbar::MOSAIC_COLOR {
            data.color = toolbar::COLORS[0];
        }
        if tool == Tool::Highlight {
            data.color = toolbar::COLORS[2];
        }
        data.selecting_text = false;
        if data.screen.is_none() {
            data.editing = true;
            toolbar::sync_pin(hwnd);
        }
        InvalidateRect(hwnd, null(), 0);
        return Ok(());
    }
    if !matches!(id, 30..=42) {
        finish_polyline(data);
    }
    let state = &mut *data.app;
    match id {
        12 => {
            data.text_selection_enabled = !data.text_selection_enabled;
            data.text_selection = TextSelection::default();
            data.selecting_text = false;
            if data.text_selection_enabled {
                data.recognition_attempted = data.recognition.is_some();
                data.recognition_error = None;
            }
        }
        9 => {
            let image = rendered(data)?;
            copy_image(hwnd, &image)?;
            if data.screen.is_some() {
                store(state, &image, "截图")?;
                DestroyWindow(hwnd);
            }
        }
        10 => {
            let image = rendered(data)?;
            if data.screen.is_some() {
                store(state, &image, "截图")?;
            }
            pin(state, image, None, data.screen.is_none())?;
            if data.screen.is_some() {
                DestroyWindow(hwnd);
            }
        }
        11 => {
            if let Some(path) = save_dialog(hwnd) {
                let image = rendered(data)?;
                save_image(&path, &image)?;
                if data.screen.is_some() {
                    store(state, &image, "截图")?;
                    DestroyWindow(hwnd);
                }
            }
        }
        13 => {
            run_ocr(hwnd, data, true)?;
        }
        14 => {
            let image = rendered(data)?;
            let gray = image::DynamicImage::ImageRgba8(image).to_luma8();
            let mut qr = rqrr::PreparedImage::prepare(gray);
            let values: Vec<String> = qr
                .detect_grids()
                .into_iter()
                .filter_map(|grid| grid.decode().ok().map(|(_, text)| text))
                .collect();
            if values.is_empty() {
                alert(hwnd, "没有识别到二维码");
            } else if let Some(value) =
                prompt(hwnd, "二维码内容（确定后复制）", &values.join("\n\n"), true)
            {
                copy_text(hwnd, &value)?;
                let url = value.trim();
                if (url.starts_with("https://") || url.starts_with("http://"))
                    && !url.contains(['\n', '\r'])
                    && confirm(hwnd, &format!("二维码已复制。打开以下网址？\n{url}"))
                {
                    ShellExecuteW(
                        hwnd,
                        wide("open").as_ptr(),
                        wide(url).as_ptr(),
                        null(),
                        null(),
                        SW_SHOWNORMAL,
                    );
                }
            }
        }
        15 => start_long(hwnd)?,
        16 => {
            settings(state, hwnd)?;
        }
        21 => {
            clear_recognition(data);
            data.polyline_active = false;
            if let Some(mark) = data.marks.pop() {
                data.redo.push(mark);
            }
        }
        22 => {
            clear_recognition(data);
            data.polyline_active = false;
            if let Some(mark) = data.redo.pop() {
                data.marks.push(mark);
            }
        }
        25 => copy_selected_text(hwnd, data)?,
        30..=37 => data.color = toolbar::COLORS[id - 30],
        38 => {
            finish_polyline(data);
            data.tool = Tool::Pen;
            data.color = toolbar::MOSAIC_COLOR;
            if data.screen.is_none() {
                data.editing = true;
                toolbar::sync_pin(hwnd);
            }
        }
        40..=42 => data.stroke = [2, 3, 6][id - 40],
        100 => {
            DestroyWindow(hwnd);
            return Ok(());
        }
        101 => {
            if let Some(path) = &data.file {
                ShellExecuteW(
                    hwnd,
                    wide("open").as_ptr(),
                    wide(&path.to_string_lossy()).as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        19 if data.screen.is_some() => {
            let r = data.selection;
            let fields = [
                Field::text("左边 X", r.left),
                Field::text("顶部 Y", r.top),
                Field::text("宽度", (r.right - r.left).max(1)),
                Field::text("高度", (r.bottom - r.top).max(1)),
                Field::check("保存为固定区域预设", false),
            ];
            if let Some(values) = form(hwnd, "精确截图尺寸", "使用当前屏幕内的像素坐标。", &fields)
            {
                let mut n = [0i32; 4];
                for i in 0..4 {
                    n[i] = values[i].parse().map_err(|_| "请输入整数")?;
                }
                if n[0] < 0
                    || n[1] < 0
                    || n[2] <= 0
                    || n[3] <= 0
                    || n[0] as i64 + n[2] as i64 > data.image.width() as i64
                    || n[1] as i64 + n[3] as i64 > data.image.height() as i64
                {
                    return Err("区域超出屏幕范围".into());
                }
                data.selection = RECT {
                    left: n[0],
                    top: n[1],
                    right: n[0] + n[2],
                    bottom: n[1] + n[3],
                };
                clear_recognition(data);
                if values[4] == "true" {
                    let screen = data.screen.unwrap();
                    let region = [n[0] + screen.left, n[1] + screen.top, n[2], n[3]];
                    if !state.settings.presets.contains(&region) {
                        state.settings.presets.push(region);
                    }
                    state.settings.presets.truncate(20);
                    write_json(&state.root.join("settings.json"), &state.settings)?;
                }
            }
        }
        20 if data.screen.is_some() => {
            clear_recognition(data);
            data.selection = RECT {
                left: 0,
                top: 0,
                right: data.image.width() as i32,
                bottom: data.image.height() as i32,
            };
            data.tool = Tool::Select;
            data.marks.clear();
            data.redo.clear();
            InvalidateRect(hwnd, null(), 0);
        }
        _ => {}
    }
    if IsWindow(hwnd) != 0 {
        InvalidateRect(hwnd, null(), 0);
        let bar = (*view(hwnd)).toolbar_window;
        if !bar.is_null() {
            InvalidateRect(bar, null(), 0);
        }
    }
    Ok(())
}
unsafe fn settings(state: &mut App, parent: HWND) -> Result<()> {
    let current = &state.settings;
    let fields = [
        Field::text("图片历史保留天数（1–3650）", current.history_days),
        Field::check("自动识别界面元素", current.detect_ui),
        Field::check("截图包含鼠标指针", current.include_cursor),
        Field::text("圆角半径（像素，0关闭）", current.radius),
        Field::check("输出添加阴影", current.shadow),
        Field::check("输出添加边框", current.border),
        Field::text(
            "固定比例（如16:9，留空自由框选）",
            current
                .fixed_ratio
                .map(|[w, h]| format!("{w}:{h}"))
                .unwrap_or_default(),
        ),
        Field::text("取色格式（HEX / RGB / HSL / HSV）", &current.color_format),
        Field::text("翻译服务（chat 或 deepl）", &current.translation.provider),
        Field::text("翻译 API 完整地址（HTTPS）", &current.translation.endpoint),
        Field::text("AI 模型名称（DeepL可留空）", &current.translation.model),
        Field::secret("翻译 API 密钥", &current.translation.api_key),
    ];
    if let Some(values) = form(
        parent,
        "截图与翻译设置",
        "OCR始终离线。翻译时仅将识别文字发送到你配置的服务。chat 使用兼容聊天 API；deepl 使用 DeepL API。",
        &fields,
    ) {
        let mut next = current.clone();
        next.history_days = values[0].parse().map_err(|_| "历史天数应为整数")?;
        next.detect_ui = values[1] == "true";
        next.include_cursor = values[2] == "true";
        next.radius = values[3].parse().map_err(|_| "圆角半径应为整数")?;
        next.shadow = values[4] == "true";
        next.border = values[5] == "true";
        next.fixed_ratio = if values[6].trim().is_empty() {
            None
        } else {
            let (w, h) = values[6].split_once(':').ok_or("比例格式应为宽:高")?;
            let w = w.trim().parse::<u32>().map_err(|_| "比例宽度无效")?;
            let h = h.trim().parse::<u32>().map_err(|_| "比例高度无效")?;
            if w == 0 || h == 0 || w > 10000 || h > 10000 {
                return Err("比例范围为1至10000".into());
            }
            Some([w, h])
        };
        next.color_format = values[7].trim().to_ascii_uppercase();
        if !["HEX", "RGB", "HSL", "HSV"].contains(&next.color_format.as_str()) {
            return Err("不支持的取色格式".into());
        }
        next.translation.provider = values[8].trim().to_lowercase();
        if !["chat", "deepl"].contains(&next.translation.provider.as_str()) {
            return Err("翻译服务应为chat或deepl".into());
        }
        next.translation.endpoint = values[9].trim().into();
        next.translation.model = values[10].trim().into();
        next.translation.api_key = values[11].trim().into();
        if !(1..=3650).contains(&next.history_days) || next.radius > 2000 {
            return Err("历史天数或圆角超出范围".into());
        }
        crate::translation::save_key(&state.root, &next.translation.api_key)?;
        write_json(&state.root.join("settings.json"), &next)?;
        state.settings = next;
    }
    Ok(())
}

unsafe fn history(state: &mut App) -> Result<()> {
    let items = History::new(&state.root, state.settings.history_days)?.list()?;
    let mut data = empty_view(RgbaImage::from_pixel(720, 420, Rgba([25, 27, 30, 255])));
    data.history = items;
    let hwnd = create_view(
        state,
        data,
        "图片历史 · 双击贴出 · Enter贴出 · Ctrl+C复制 · Esc关闭",
        RECT {
            left: 200,
            top: 180,
            right: 920,
            bottom: 620,
        },
        false,
    );
    if hwnd.is_null() {
        return Err("无法打开历史窗口".into());
    }
    let data = &mut *view(hwnd);
    child(
        hwnd,
        "STATIC",
        "图片历史 · 双击 / Enter 贴出 · Ctrl+C 复制 · Esc 关闭",
        0,
        221,
        [12, 8, 640, 28],
    );
    child(hwnd, "BUTTON", "×", WS_TABSTOP, 220, [670, 4, 36, 32]);
    data.list = child(
        hwnd,
        "LISTBOX",
        "",
        WS_VSCROLL | LBS_NOTIFY as u32 | LBS_OWNERDRAWFIXED as u32 | LBS_HASSTRINGS as u32,
        1,
        [0, 42, 720, 398],
    );
    for item in &data.history {
        SendMessageW(
            data.list,
            LB_ADDSTRING,
            0,
            wide(&format!(
                "{} · {}×{} · {}",
                item.source, item.width, item.height, item.created
            ))
            .as_ptr() as isize,
        );
    }
    SendMessageW(data.list, LB_SETCURSEL, 0, 0);
    SetFocus(data.list);
    Ok(())
}

pub fn run(initial: Option<Invocation>, smoke: bool) -> Result<()> {
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(null(), 0);
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module = GetModuleHandleW(null());
        let class = wide("ptools.capture.controller");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(controller_proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let mut state = Box::new(App {
            hwnd: null_mut(),
            root: PathBuf::new(),
            settings: Default::default(),
            windows: HashSet::new(),
            busy: false,
            delayed: false,
        });
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("ptools capture controller").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            module,
            (&mut *state as *mut App).cast(),
        );
        if hwnd.is_null() {
            return Err("无法启动截图插件".into());
        }
        state.hwnd = hwnd;
        if let Some(request) = initial {
            PostMessageW(
                hwnd,
                WM_INVOKE,
                0,
                Box::into_raw(Box::new(request)) as isize,
            );
        } else {
            read_invocations(hwnd);
        }
        if smoke {
            SetTimer(hwnd, 99, 1200, None);
        }
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN && GetDlgCtrlID(message.hwnd) == 1 {
                let parent = GetParent(message.hwnd);
                if !view(parent).is_null() && !(*view(parent)).list.is_null() {
                    if message.wParam == VK_RETURN as usize {
                        PostMessageW(parent, WM_COMMAND, 1 | ((LBN_DBLCLK as usize) << 16), 0);
                        continue;
                    }
                    if message.wParam == 0x43 && GetKeyState(VK_CONTROL as i32) < 0 {
                        let data = &*view(parent);
                        let index = SendMessageW(data.list, LB_GETCURSEL, 0, 0) as usize;
                        if let Some(item) = data.history.get(index)
                            && let Ok(store) =
                                History::new(&(*data.app).root, (*data.app).settings.history_days)
                            && let Ok(image) = store.load(&item.id, false)
                        {
                            let _ = copy_image(parent, &image);
                        }
                        continue;
                    }
                    if message.wParam == VK_ESCAPE as usize {
                        PostMessageW(parent, WM_CLOSE, 0, 0);
                        continue;
                    }
                }
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        for window in state.windows.clone() {
            DestroyWindow(window as HWND);
        }
        DestroyWindow(hwnd);
        windows_sys::Win32::System::Com::CoUninitialize();
        Ok(())
    }
}
unsafe extern "system" fn controller_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let state = app(hwnd);
    if state.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    match message {
        WM_INVOKE => {
            if l == 0 {
                PostQuitMessage(0);
                return 0;
            }
            let request = Box::from_raw(l as *mut Invocation);
            (*state).busy = true;
            let result = (|| -> Result<()> {
                if (*state).root != request.data_dir {
                    if !(*state).windows.is_empty() {
                        return Err("当前插件正在使用另一个数据目录".into());
                    }
                    (*state).root = request.data_dir.clone();
                    fs::create_dir_all(&(*state).root).map_err(|e| e.to_string())?;
                    (*state).settings = read_json(&(*state).root.join("settings.json"))?;
                    if let Some(key) = crate::translation::load_key(&(*state).root) {
                        (*state).settings.translation.api_key = key;
                    }
                }
                match request.action.as_str() {
                    "capture" => capture(&mut *state, false, false),
                    "custom" => capture(&mut *state, true, false),
                    "quick" => match request.region {
                        Some(region) => quick_region(&mut *state, region),
                        None => capture(&mut *state, false, true),
                    },
                    "pin" => {
                        let (image, path) = clipboard_image(hwnd)?;
                        pin(&mut *state, image, path, true)?;
                        Ok(())
                    }
                    "history" => history(&mut *state),
                    "settings" => settings(&mut *state, hwnd),
                    _ => Err("未知截图动作".into()),
                }
            })();
            if let Err(e) = result {
                alert(hwnd, &e);
            }
            (*state).busy = false;
            PostMessageW(hwnd, WM_IDLE, 0, 0);
            return 0;
        }
        WM_IDLE => {
            if !(*state).busy && !(*state).delayed && (*state).windows.is_empty() {
                SetTimer(hwnd, 5, 150, None);
            }
            return 0;
        }
        WM_TIMER if w == 5 => {
            KillTimer(hwnd, 5);
            if !(*state).busy && !(*state).delayed && (*state).windows.is_empty() {
                PostQuitMessage(0);
            }
            return 0;
        }
        WM_TIMER if w == 4 => {
            KillTimer(hwnd, 4);
            (*state).delayed = false;
            let result = if let Some([x, y, width, height]) = (*state).settings.region {
                let region = RECT {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + height,
                };
                screenshot(region).and_then(|mut image| {
                    if (*state).settings.include_cursor {
                        draw_cursor(&mut image, region);
                    }
                    let mut data = empty_view(image);
                    data.app = state;
                    data.screen = Some(region);
                    data.selection = RECT {
                        left: 0,
                        top: 0,
                        right: width,
                        bottom: height,
                    };
                    let image = rendered(&data)?;
                    store(&*state, &image, "截图")?;
                    copy_image(hwnd, &image)
                })
            } else {
                capture(&mut *state, false, false)
            };
            if let Err(e) = result {
                alert(hwnd, &e);
            }
            PostMessageW(hwnd, WM_IDLE, 0, 0);
            return 0;
        }
        WM_OCR => {
            let work = Box::from_raw(l as *mut OcrWork);
            let (window, token) = (work.window, work.token);
            if (*state).windows.contains(&window)
                && !view(window as HWND).is_null()
                && (*view(window as HWND)).token == token
            {
                let ptr = Box::into_raw(work);
                SendMessageW(window as HWND, WM_OCR, token, ptr as isize);
            }
            return 0;
        }
        WM_TIMER if w == 99 => {
            for window in (*state).windows.clone() {
                DestroyWindow(window as HWND);
            }
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, w, l)
}
unsafe extern "system" fn view_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let ptr = view(hwnd);
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    let data = &mut *ptr;
    match message {
        WM_CREATE => {
            data.font = CreateFontW(
                -((15 * GetDpiForWindow(hwnd).max(96) / 96) as i32),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                0,
                0,
                wide("Microsoft YaHei").as_ptr(),
            );
            SetTimer(hwnd, 6, 250, None);
            return 0;
        }
        WM_PAINT | WM_PRINTCLIENT => {
            let mut paint: PAINTSTRUCT = zeroed();
            let target_dc = if message == WM_PRINTCLIENT {
                w as HDC
            } else {
                BeginPaint(hwnd, &mut paint)
            };
            let mut client = RECT::default();
            GetClientRect(hwnd, &mut client);
            if data.long.is_some() {
                draw_long_frame(target_dc, client, data.selection);
                if message == WM_PAINT {
                    EndPaint(hwnd, &paint);
                }
                return 0;
            }
            let dc = CreateCompatibleDC(target_dc);
            let bitmap =
                CreateCompatibleBitmap(target_dc, client.right.max(1), client.bottom.max(1));
            let previous_bitmap = SelectObject(dc, bitmap);
            let saved = SaveDC(dc);
            if data.screen.is_none() && !data.marks.is_empty() {
                if let Ok(image) = render_image(data, false) {
                    draw_image(dc, &image, [2, 2, client.right - 4, client.bottom - 4]);
                }
            } else {
                let inset = if data.screen.is_none() && data.long.is_none() && data.list.is_null() {
                    2
                } else {
                    0
                };
                draw_image(
                    dc,
                    &data.image,
                    [
                        inset,
                        inset,
                        client.right - inset * 2,
                        client.bottom - inset * 2,
                    ],
                );
            }
            if data.screen.is_none() && data.long.is_none() && data.list.is_null() {
                let brush = CreateSolidBrush(theme::BORDER);
                FrameRect(dc, &client, brush);
                DeleteObject(brush);
                let inner = RECT {
                    left: 1,
                    top: 1,
                    right: client.right - 1,
                    bottom: client.bottom - 1,
                };
                FrameRect(dc, &inner, theme::surface_brush());
            }
            if data.screen.is_some() {
                let selection = if data.selection.right > data.selection.left {
                    data.selection
                } else {
                    data.hover
                };
                let shade = CreateCompatibleDC(dc);
                let black = CreateCompatibleBitmap(dc, 1, 1);
                let previous = SelectObject(shade, black);
                PatBlt(shade, 0, 0, 1, 1, BLACKNESS);
                for r in [
                    RECT {
                        left: 0,
                        top: 0,
                        right: client.right,
                        bottom: selection.top,
                    },
                    RECT {
                        left: 0,
                        top: selection.bottom,
                        right: client.right,
                        bottom: client.bottom,
                    },
                    RECT {
                        left: 0,
                        top: selection.top,
                        right: selection.left,
                        bottom: selection.bottom,
                    },
                    RECT {
                        left: selection.right,
                        top: selection.top,
                        right: client.right,
                        bottom: selection.bottom,
                    },
                ] {
                    if r.right > r.left && r.bottom > r.top {
                        GdiAlphaBlend(
                            dc,
                            r.left,
                            r.top,
                            r.right - r.left,
                            r.bottom - r.top,
                            shade,
                            0,
                            0,
                            1,
                            1,
                            BLENDFUNCTION {
                                BlendOp: AC_SRC_OVER as u8,
                                BlendFlags: 0,
                                SourceConstantAlpha: 100,
                                AlphaFormat: 0,
                            },
                        );
                    }
                }
                SelectObject(shade, previous);
                DeleteObject(black);
                DeleteDC(shade);
                let pen = CreatePen(PS_SOLID, 2, theme::ACCENT);
                let old_pen = SelectObject(dc, pen);
                let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                Rectangle(
                    dc,
                    selection.left,
                    selection.top,
                    selection.right,
                    selection.bottom,
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(pen);
                if !data.marks.is_empty()
                    && let Ok(image) = render_image(data, false)
                {
                    draw_image(
                        dc,
                        &image,
                        [
                            data.selection.left,
                            data.selection.top,
                            data.selection.right - data.selection.left,
                            data.selection.bottom - data.selection.top,
                        ],
                    );
                }
                if data.selection.right > data.selection.left {
                    let brush = CreateSolidBrush(theme::ACCENT);
                    FrameRect(dc, &selection, brush);
                    let mid_x = (selection.left + selection.right) / 2;
                    let mid_y = (selection.top + selection.bottom) / 2;
                    for [x, y] in [
                        [selection.left, selection.top],
                        [mid_x, selection.top],
                        [selection.right, selection.top],
                        [selection.left, mid_y],
                        [selection.right, mid_y],
                        [selection.left, selection.bottom],
                        [mid_x, selection.bottom],
                        [selection.right, selection.bottom],
                    ] {
                        let r = RECT {
                            left: x - 3,
                            top: y - 3,
                            right: x + 4,
                            bottom: y + 4,
                        };
                        FillRect(dc, &r, theme::surface_brush());
                        SetTextColor(dc, theme::TEXT);
                        SetBkMode(dc, TRANSPARENT as i32);
                        FrameRect(dc, &r, brush);
                    }
                    DeleteObject(brush);
                    let label = format!(
                        "{} × {}",
                        selection.right - selection.left,
                        selection.bottom - selection.top
                    );
                    let mut r = RECT {
                        left: selection.left,
                        top: (selection.top - 26).max(0),
                        right: selection.left + 160,
                        bottom: selection.top.max(26),
                    };
                    FillRect(dc, &r, GetStockObject(BLACK_BRUSH) as HBRUSH);
                    SetBkMode(dc, TRANSPARENT as i32);
                    SetTextColor(dc, 0xffffff);
                    SelectObject(dc, data.font);
                    DrawTextW(
                        dc,
                        wide(&label).as_ptr(),
                        -1,
                        &mut r,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
                let p = data.point;
                if (data.selection.right <= data.selection.left || data.tool == Tool::PickColor)
                    && p[0] >= 0
                    && p[1] >= 0
                    && p[0] < data.image.width() as i32
                    && p[1] < data.image.height() as i32
                {
                    let pixel = data.image.get_pixel(p[0] as u32, p[1] as u32);
                    let mut r = RECT {
                        left: (p[0] + 18).min(client.right - 280).max(0),
                        top: (p[1] + 18).min(client.bottom - 140).max(0),
                        right: 0,
                        bottom: 0,
                    };
                    r.right = r.left + 280;
                    r.bottom = r.top + 136;
                    FillRect(dc, &r, theme::surface_brush());
                    SetTextColor(dc, theme::TEXT);
                    SetBkMode(dc, TRANSPARENT as i32);
                    let zoom = image::imageops::crop_imm(
                        &data.image,
                        (p[0] - 5).max(0) as u32,
                        (p[1] - 5).max(0) as u32,
                        11.min(data.image.width() - (p[0] - 5).max(0) as u32),
                        11.min(data.image.height() - (p[1] - 5).max(0) as u32),
                    )
                    .to_image();
                    draw_pixels(dc, &zoom, [r.left + 8, r.top + 8, 77, 77]);
                    let cx = (p[0] - (p[0] - 5).max(0)) * 7 + r.left + 8;
                    let cy = (p[1] - (p[1] - 5).max(0)) * 7 + r.top + 8;
                    FrameRect(
                        dc,
                        &RECT {
                            left: cx,
                            top: cy,
                            right: cx + 7,
                            bottom: cy + 7,
                        },
                        GetStockObject(BLACK_BRUSH) as HBRUSH,
                    );
                    let label = format!(
                        "{}×{}\n{}\n单击复制颜色",
                        selection.right - selection.left,
                        selection.bottom - selection.top,
                        color_value(*pixel, &(*data.app).settings.color_format)
                    );
                    r.left += 96;
                    SelectObject(dc, data.font);
                    SetBkMode(dc, TRANSPARENT as i32);
                    DrawTextW(dc, wide(&label).as_ptr(), -1, &mut r, DT_LEFT);
                }
            }
            if data.text_selection_enabled {
                let (sx, sy, padding) = if data.screen.is_some() {
                    (1.0, 1.0, 0)
                } else {
                    (
                        (client.right - 4) as f32 / data.image.width() as f32,
                        (client.bottom - 4) as f32 / data.image.height() as f32,
                        2,
                    )
                };
                let brush = CreateSolidBrush(theme::ACCENT);
                for &[x, y, width, height] in &data.text_selection.bounds {
                    let x = x + data.recognition_origin[0] as f32;
                    let y = y + data.recognition_origin[1] as f32;
                    let r = RECT {
                        left: padding + (x * sx) as i32,
                        top: padding + (y * sy) as i32,
                        right: padding + ((x + width) * sx).ceil() as i32,
                        bottom: padding + ((y + height) * sy).ceil() as i32,
                    };
                    FrameRect(dc, &r, brush);
                }
                DeleteObject(brush);
            }
            if toolbar::visible(data) {
                toolbar::draw(dc, data, &toolbar::layout(hwnd, data));
            }
            RestoreDC(dc, saved);
            BitBlt(
                target_dc,
                0,
                0,
                client.right,
                client.bottom,
                dc,
                0,
                0,
                SRCCOPY,
            );
            SelectObject(dc, previous_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            if message == WM_PAINT {
                EndPaint(hwnd, &paint);
            }
            return 0;
        }
        WM_LBUTTONDOWN => {
            if data.long.is_some() {
                return 0;
            }
            let p = point(l);
            text_editor::finish(hwnd, data, true);
            let bar = toolbar::layout(hwnd, data);
            if toolbar::visible(data) && contains(bar.bounds, p) {
                if let Err(e) = toolbar::click(hwnd, hwnd, p) {
                    alert(hwnd, &e);
                }
                if IsWindow(hwnd) != 0 {
                    InvalidateRect(hwnd, null(), 0);
                }
                return 0;
            }
            if data.screen.is_some() && data.tool != Tool::Select && !contains(data.selection, p) {
                return 0;
            }
            if data.working {
                return 0;
            }
            if text_hit(hwnd, data, p) {
                data.dragging = true;
                data.selecting_text = true;
                data.start = p;
                data.point = p;
                select_text(hwnd, data, p);
                SetCapture(hwnd);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            data.text_selection = TextSelection::default();
            if data.tool == Tool::Text {
                text_editor::begin(hwnd, data, p);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.tool == Tool::PickColor {
                data.point = p;
                if let Err(e) = copy_picked_color(hwnd, data) {
                    alert(hwnd, &e);
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.tool == Tool::Polyline {
                clear_recognition(data);
                let p = image_point(hwnd, data, p);
                if data.polyline_active {
                    if let Some(mark) = data.marks.last_mut() {
                        *mark.points.last_mut().unwrap() = p;
                        mark.points.push(p);
                    }
                } else {
                    data.marks.push(Mark {
                        tool: Tool::Polyline,
                        points: vec![p, p],
                        text: String::new(),
                        color: data.color,
                        width: data.stroke,
                    });
                    data.redo.clear();
                    data.polyline_active = true;
                }
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.screen.is_some() || data.tool != Tool::Select {
                clear_recognition(data);
                data.dragging = true;
                data.start = p;
                SetCapture(hwnd);
                if data.tool == Tool::Select && data.screen.is_some() {
                    data.selection_drag = selection_hit(
                        data.selection,
                        p,
                        (6 * GetDpiForWindow(hwnd).max(96) / 96) as i32,
                    );
                    data.selection_origin = data.selection;
                    if data.selection_drag == 0 {
                        data.selection = rect(p, p);
                        data.marks.clear();
                        data.redo.clear();
                    }
                } else {
                    let mut label = String::new();
                    if data.tool == Tool::Number {
                        data.dragging = false;
                        ReleaseCapture();
                        label = (data.marks.iter().filter(|m| m.tool == Tool::Number).count() + 1)
                            .to_string();
                    }
                    data.redo.clear();
                    let image_p = image_point(hwnd, data, p);
                    data.marks.push(Mark {
                        tool: data.tool,
                        points: if matches!(data.tool, Tool::Pen | Tool::Highlight) {
                            vec![image_p]
                        } else {
                            vec![image_p, image_p]
                        },
                        text: label,
                        color: data.color,
                        width: data.stroke,
                    });
                }
            } else {
                ReleaseCapture();
                SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
            }
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_MOUSEMOVE => {
            if data.long.is_some() {
                return 0;
            }
            let p = point(l);
            let image_p = image_point(hwnd, data, p);
            data.point = p;
            data.hover_button = if toolbar::visible(data) {
                toolbar::layout(hwnd, data).hit(p)
            } else {
                None
            };
            if data.polyline_active
                && toolbar::visible(data)
                && contains(toolbar::layout(hwnd, data).bounds, p)
            {
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.polyline_active
                && let Some(mark) = data.marks.last_mut()
            {
                *mark.points.last_mut().unwrap() = if data.screen.is_some() {
                    [
                        image_p[0].clamp(data.selection.left, data.selection.right - 1),
                        image_p[1].clamp(data.selection.top, data.selection.bottom - 1),
                    ]
                } else {
                    image_p
                };
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.dragging {
                if data.selecting_text {
                    select_text(hwnd, data, p);
                } else if data.tool == Tool::Select && data.screen.is_some() {
                    if data.selection_drag != 0 {
                        adjust_selection(data, p);
                        InvalidateRect(hwnd, null(), 0);
                        return 0;
                    }
                    data.selection = rect(
                        data.start,
                        [
                            p[0].clamp(0, data.image.width() as i32),
                            p[1].clamp(0, data.image.height() as i32),
                        ],
                    );
                    let ratio = (*data.app).settings.fixed_ratio.or_else(|| {
                        if GetKeyState(VK_SHIFT as i32) < 0 {
                            Some([1, 1])
                        } else {
                            None
                        }
                    });
                    if let Some([width, height]) = ratio {
                        data.selection.bottom = data.selection.top
                            + ((data.selection.right - data.selection.left) as i64 * height as i64
                                / width.max(1) as i64)
                                .min(data.image.height() as i64 - data.selection.top as i64)
                                as i32;
                    }
                } else if !data.selecting_text
                    && let Some(mark) = data.marks.last_mut()
                {
                    let image_p = if data.screen.is_some() {
                        [
                            image_p[0].clamp(data.selection.left, data.selection.right - 1),
                            image_p[1].clamp(data.selection.top, data.selection.bottom - 1),
                        ]
                    } else {
                        image_p
                    };
                    if mark.tool == Tool::Pen || mark.tool == Tool::Highlight {
                        mark.points.push(image_p);
                    } else {
                        *mark.points.last_mut().unwrap() = image_p;
                    }
                }
            } else if let Some(screen) = data.screen
                && data.selection.right <= data.selection.left
                && data.detect_at.elapsed().as_millis() > 80
            {
                data.hover = detect(p, screen, (*data.app).settings.detect_ui);
                data.detect_at = Instant::now();
            }
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_SETCURSOR if l as u32 & 0xffff == HTCLIENT => {
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor);
            ScreenToClient(hwnd, &mut cursor);
            let p = [cursor.x, cursor.y];
            let over_toolbar =
                toolbar::visible(data) && contains(toolbar::layout(hwnd, data).bounds, p);
            SetCursor(LoadCursorW(
                null_mut(),
                if over_toolbar {
                    IDC_ARROW
                } else if data.working {
                    IDC_WAIT
                } else if text_hit(hwnd, data, p) || data.selecting_text || data.tool == Tool::Text
                {
                    IDC_IBEAM
                } else if data.screen.is_some() || data.tool != Tool::Select {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                },
            ));
            return 1;
        }
        WM_LBUTTONUP => {
            if data.long.is_some() {
                return 0;
            }
            let p = point(l);
            if data.dragging && data.tool != Tool::Select && !data.selecting_text {
                let mut end = image_point(hwnd, data, p);
                if data.screen.is_some() {
                    end = [
                        end[0].clamp(data.selection.left, data.selection.right - 1),
                        end[1].clamp(data.selection.top, data.selection.bottom - 1),
                    ];
                }
                if let Some(mark) = data.marks.last_mut() {
                    if matches!(mark.tool, Tool::Pen | Tool::Highlight) {
                        if mark.points.last() != Some(&end) {
                            mark.points.push(end);
                        }
                    } else {
                        *mark.points.last_mut().unwrap() = end;
                    }
                }
            }
            let finish_quick =
                data.quick && data.dragging && data.tool == Tool::Select && !data.selecting_text;
            if data.dragging
                && data.tool == Tool::Select
                && !data.selecting_text
                && data.screen.is_some()
                && data.selection_drag != 0
            {
                adjust_selection(data, p);
            }
            if data.dragging
                && data.tool == Tool::Select
                && !data.selecting_text
                && data.screen.is_some()
                && data.selection_drag == 0
            {
                let end = [
                    p[0].clamp(0, data.image.width() as i32),
                    p[1].clamp(0, data.image.height() as i32),
                ];
                data.selection = rect(data.start, end);
                if let Some([width, height]) = (*data.app).settings.fixed_ratio {
                    data.selection.bottom = (data.selection.top
                        + ((data.selection.right - data.selection.left) as i64 * height as i64
                            / width.max(1) as i64) as i32)
                        .min(data.image.height() as i32);
                } else if GetKeyState(VK_SHIFT as i32) < 0 {
                    data.selection.bottom = (data.selection.top + data.selection.right
                        - data.selection.left)
                        .min(data.image.height() as i32);
                }
            }
            if data.dragging
                && data.tool == Tool::Select
                && !data.selecting_text
                && data.screen.is_some()
                && data.selection_drag == 0
                && (p[0] - data.start[0]).abs() + (p[1] - data.start[1]).abs() < 4
            {
                data.selection = data.hover;
            }
            if finish_quick
                && data.selection.right - data.selection.left >= 4
                && data.selection.bottom - data.selection.top >= 4
            {
                let screen = data.screen.unwrap();
                let region = RECT {
                    left: screen.left + data.selection.left,
                    top: screen.top + data.selection.top,
                    right: screen.left + data.selection.right,
                    bottom: screen.top + data.selection.bottom,
                };
                let parent = data.app;
                let result = rendered(data);
                DestroyWindow(hwnd);
                if let Err(error) = result.and_then(|image| quick_pin(&mut *parent, image, region))
                {
                    alert((*parent).hwnd, &error);
                }
                return 0;
            }
            if data.selecting_text {
                select_text(hwnd, data, p);
                if let Err(e) = copy_selected_text(hwnd, data) {
                    alert(hwnd, &e);
                }
                data.selecting_text = false;
            }
            data.dragging = false;
            ReleaseCapture();
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            if data.long.is_some() {
                return 0;
            }
            if data.polyline_active {
                finish_polyline(data);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            let p = point(l);
            if text_hit(hwnd, data, p) {
                data.start = p;
                select_text(hwnd, data, p);
                let _ = copy_selected_text(hwnd, data);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if data.tool != Tool::Select || data.editing {
                return 0;
            }
            if data.screen.is_some() {
                if let Err(e) = command(hwnd, 9) {
                    alert(hwnd, &e);
                }
            } else {
                DestroyWindow(hwnd);
            }
            return 0;
        }
        WM_MOUSEWHEEL if data.screen.is_none() && data.list.is_null() => {
            let delta = ((w >> 16) as u16 as i16) as i32;
            if GetKeyState(VK_CONTROL as i32) < 0 {
                data.alpha =
                    (data.alpha as i32 + if delta > 0 { 16 } else { -16 }).clamp(32, 255) as u8;
                SetLayeredWindowAttributes(hwnd, 0, data.alpha, LWA_ALPHA);
            } else {
                let mut r = RECT::default();
                GetWindowRect(hwnd, &mut r);
                let factor = if delta > 0 { 1.1 } else { 0.9 };
                let width = ((r.right - r.left) as f32 * factor).clamp(32.0, 16000.0) as i32;
                let height = ((r.bottom - r.top) as f32 * factor).clamp(32.0, 16000.0) as i32;
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            return 0;
        }
        WM_MOVE | WM_SIZE => {
            if data.long.is_some() {
                long_panel::sync(hwnd);
            } else {
                toolbar::sync_pin(hwnd);
            }
            text_editor::resize(hwnd, data);
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_DPICHANGED => {
            let mut font: LOGFONTW = zeroed();
            GetObjectW(
                data.font,
                size_of::<LOGFONTW>() as i32,
                (&mut font as *mut LOGFONTW).cast(),
            );
            font.lfHeight = -((15 * (w as u32 & 0xffff) / 96) as i32);
            let next = CreateFontIndirectW(&font);
            if !next.is_null() {
                DeleteObject(data.font);
                data.font = next;
            }
            if data.screen.is_none() {
                let r = &*(l as *const RECT);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_CAPTURECHANGED => {
            data.dragging = false;
            data.selecting_text = false;
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_CONTEXTMENU => {
            if data.long.is_some() {
                return 0;
            }
            let popup = CreatePopupMenu();
            for (id, label) in [
                (9, "复制图片"),
                (10, "另建贴图"),
                (11, "另存为"),
                (12, "文字框选开关"),
                (13, "翻译"),
                (14, "识别二维码"),
                (1, "矩形标注"),
                (2, "椭圆标注"),
                (3, "箭头标注"),
                (4, "画笔"),
                (5, "荧光笔"),
                (6, "文字标注"),
                (7, "序号"),
                (38, "画笔马赛克"),
                (24, "取色"),
                (25, "复制选中文字"),
                (17, "直线标注"),
                (18, "折线标注"),
                (0, "框选 / 移动"),
                (21, "撤销"),
                (22, "重做"),
                (19, "精确尺寸"),
                (16, "设置"),
                (100, "关闭"),
            ] {
                AppendMenuW(
                    popup,
                    MF_STRING
                        | if id == 12 && data.text_selection_enabled {
                            MF_CHECKED
                        } else {
                            0
                        }
                        | if toolbar::enabled(data, id) {
                            0
                        } else {
                            MF_GRAYED
                        },
                    id,
                    wide(label).as_ptr(),
                );
            }
            if data.file.is_some() {
                AppendMenuW(popup, MF_STRING, 101, wide("打开原文件").as_ptr());
            }
            let mut p = POINT::default();
            GetCursorPos(&mut p);
            let id = TrackPopupMenu(
                popup,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                p.x,
                p.y,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(popup);
            if id != 0
                && let Err(e) = command(hwnd, id as usize)
            {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_COMMAND
            if w >> 16 == 0
                && ((0..=25).contains(&(w & 0xffff))
                    || (30..=42).contains(&(w & 0xffff))
                    || w == 100
                    || w == 101) =>
        {
            if let Err(e) = command(hwnd, w & 0xffff) {
                alert(hwnd, &e);
            }
            if IsWindow(hwnd) != 0 {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_COMMAND if w & 0xffff == 220 => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_KEYDOWN => {
            let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
            let shift = GetKeyState(VK_SHIFT as i32) < 0;
            if data.long.is_some() {
                let id = match w as u16 {
                    VK_ESCAPE => 205,
                    VK_RETURN => 201,
                    0x43 if ctrl => 201,
                    0x54 if ctrl => 202,
                    0x53 if ctrl => 203,
                    _ => return 0,
                };
                if let Err(e) = finish_long(hwnd, id) {
                    alert(hwnd, &e);
                }
                return 0;
            }
            if data.text_selection_enabled {
                match w as u16 {
                    0x43 if ctrl && !data.text_selection.text.is_empty() => {
                        if !data.working
                            && let Err(e) = copy_selected_text(hwnd, data)
                        {
                            alert(hwnd, &e);
                        }
                        return 0;
                    }
                    VK_RETURN if !data.text_selection.text.is_empty() => {
                        if !data.working
                            && let Err(e) = copy_selected_text(hwnd, data)
                        {
                            alert(hwnd, &e);
                        }
                        return 0;
                    }
                    0x41 if ctrl
                        && (data.screen.is_none() || !data.text_selection.text.is_empty()) =>
                    {
                        if !data.working
                            && let Some(recognition) = &data.recognition
                        {
                            data.text_selection = recognition.select_all();
                        }
                        InvalidateRect(hwnd, null(), 0);
                        if !data.toolbar_window.is_null() {
                            InvalidateRect(data.toolbar_window, null(), 0);
                        }
                        return 0;
                    }
                    _ => {}
                }
            }
            let id = match w as u16 {
                VK_ESCAPE => {
                    if data.polyline_active {
                        finish_polyline(data);
                        InvalidateRect(hwnd, null(), 0);
                        return 0;
                    }
                    DestroyWindow(hwnd);
                    return 0;
                }
                VK_RETURN if data.polyline_active => {
                    finish_polyline(data);
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }
                VK_RETURN => 9,
                VK_SPACE if data.screen.is_none() && !data.working => {
                    finish_polyline(data);
                    data.editing = !data.editing;
                    data.tool = if data.editing {
                        if data.group_tools[0] == 2 {
                            Tool::Ellipse
                        } else {
                            toolbar::remember_tool(data, 1);
                            Tool::Rect
                        }
                    } else {
                        Tool::Select
                    };
                    toolbar::sync_pin(hwnd);
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }
                0x43 if ctrl => 9,
                0x54 if ctrl => 10,
                0x53 if ctrl => 11,
                0x43 if shift => 12,
                0x51 if ctrl => 13,
                0x51 => 14,
                0x41 if ctrl && data.screen.is_some() => 20,
                0x5a if ctrl && shift => 22,
                0x5a if ctrl => 21,
                0x59 if ctrl => 22,
                0x56 => 0,
                0x52 => 1,
                0x45 => 2,
                0x41 => 3,
                0x50 => 4,
                0x48 => 5,
                0x54 => 6,
                0x4e => 7,
                0x4d => 38,
                0x4c => 17,
                0x46 => 18,
                0x43 => 24,
                VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN
                    if data.screen.is_some() && !data.working && data.tool == Tool::Select =>
                {
                    clear_recognition(data);
                    let dx = if w as u16 == VK_LEFT {
                        -1
                    } else if w as u16 == VK_RIGHT {
                        1
                    } else {
                        0
                    };
                    let dy = if w as u16 == VK_UP {
                        -1
                    } else if w as u16 == VK_DOWN {
                        1
                    } else {
                        0
                    };
                    if shift || ctrl {
                        data.selection.right += dx;
                        data.selection.bottom += dy;
                    } else {
                        data.selection.left += dx;
                        data.selection.right += dx;
                        data.selection.top += dy;
                        data.selection.bottom += dy;
                    }
                    let width = data.image.width() as i32;
                    let height = data.image.height() as i32;
                    data.selection.left = data.selection.left.clamp(0, width - 1);
                    data.selection.top = data.selection.top.clamp(0, height - 1);
                    data.selection.right =
                        data.selection.right.clamp(data.selection.left + 1, width);
                    data.selection.bottom =
                        data.selection.bottom.clamp(data.selection.top + 1, height);
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }
                _ => return DefWindowProcW(hwnd, message, w, l),
            };
            if let Err(e) = command(hwnd, id) {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_OCR => {
            data.working = false;
            data.recognition_pending = false;
            let work = Box::from_raw(l as *mut OcrWork);
            if work.generation != data.recognition_generation {
                return 0;
            }
            match work.result {
                Ok((recognition, translated)) => {
                    if let Some(translated) = translated {
                        let _ = prompt(
                            hwnd,
                            "翻译结果",
                            &format!("原文：\n{}\n\n译文：\n{}", recognition.text, translated),
                            true,
                        );
                    } else {
                        data.recognition_origin = work.origin;
                        data.recognition = Some(recognition);
                        data.text_selection = TextSelection::default();
                        if data.screen.is_none() {
                            toolbar::sync_pin(hwnd);
                        }
                    }
                }
                Err(e) if work.translate => alert(hwnd, &e),
                Err(e) => data.recognition_error = Some(e),
            }
            InvalidateRect(hwnd, null(), 0);
            if !data.toolbar_window.is_null() {
                InvalidateRect(data.toolbar_window, null(), 0);
            }
            return 0;
        }
        WM_TIMER if w == 2 => {
            if data.working
                || data.recognition_pending
                || data.selecting_text
                || !data.text_selection.text.is_empty()
                || data.text_editor.is_some()
            {
                return 0;
            }
            clear_recognition(data);
            if let Some(animation) = &mut data.animation {
                if let Some(Ok(frame)) = animation.frames.next() {
                    let (n, d) = frame.delay().numer_denom_ms();
                    data.image = frame.into_buffer();
                    SetTimer(hwnd, 2, (n / d.max(1)).clamp(20, 10000), None);
                    InvalidateRect(hwnd, null(), 0);
                } else if let Ok(next) = frames(&animation.path) {
                    animation.frames = next;
                } else {
                    KillTimer(hwnd, 2);
                }
            }
            return 0;
        }
        WM_TIMER if w == 3 => {
            sample_long(hwnd);
            return 0;
        }
        WM_TIMER if w == 6 => {
            if data.text_selection_enabled
                && !data.recognition_pending
                && !data.recognition_attempted
                && !data.dragging
                && !data.polyline_active
                && data.text_editor.is_none()
                && data.long.is_none()
                && data.list.is_null()
                && (data.screen.is_none()
                    || data.selection.right > data.selection.left
                        && data.selection.bottom > data.selection.top)
                && let Err(e) = run_ocr(hwnd, data, false)
            {
                data.recognition_attempted = true;
                data.recognition_error = Some(e);
            }
            return 0;
        }
        WM_COMMIT_TEXT => {
            if l == 0
                || data
                    .text_editor
                    .as_ref()
                    .is_some_and(|e| e.hwnd == l as HWND)
            {
                text_editor::finish(hwnd, data, w != 0);
            }
            return 0;
        }
        WM_COMMAND if w & 0xffff == 230 && (w >> 16) as u32 == EN_CHANGE => {
            text_editor::resize(hwnd, data);
            return 0;
        }
        WM_CTLCOLOREDIT => return text_editor::colors(w as HDC, data),
        WM_MEASUREITEM => {
            if toolbar::measure_menu(&mut *(l as *mut MEASUREITEMSTRUCT)) {
                return 1;
            }
            (*(l as *mut MEASUREITEMSTRUCT)).itemHeight = 118;
            return 1;
        }
        WM_DRAWITEM => {
            let item = &*(l as *const DRAWITEMSTRUCT);
            if toolbar::draw_menu(data, item) {
                return 1;
            }
            if let Some(history) = data.history.get(item.itemID as usize) {
                FillRect(
                    item.hDC,
                    &item.rcItem,
                    if item.itemState & ODS_SELECTED != 0 {
                        theme::selection_brush()
                    } else {
                        theme::background_brush()
                    },
                );
                if let Ok(store) =
                    History::new(&(*data.app).root, (*data.app).settings.history_days)
                    && let Ok(image) = store.load_item(history, true)
                {
                    let scale = (160.0 / image.width() as f32).min(108.0 / image.height() as f32);
                    draw_image(
                        item.hDC,
                        &image,
                        [
                            item.rcItem.left + 8,
                            item.rcItem.top + 5,
                            (image.width() as f32 * scale) as i32,
                            (image.height() as f32 * scale) as i32,
                        ],
                    );
                }
                let mut r = item.rcItem;
                r.left += 184;
                SelectObject(item.hDC, SendMessageW(data.list, WM_GETFONT, 0, 0) as HFONT);
                SetBkMode(item.hDC, TRANSPARENT as i32);
                SetTextColor(item.hDC, theme::TEXT);
                r.top += 20;
                r.right -= 16;
                DrawTextW(
                    item.hDC,
                    wide(&format!(
                        "{} · {}×{}\n{}",
                        history.source,
                        history.width,
                        history.height,
                        relative_age(history.created)
                    ))
                    .as_ptr(),
                    -1,
                    &mut r,
                    DT_LEFT,
                );
                if item.itemState & ODS_SELECTED != 0 {
                    FrameRect(item.hDC, &item.rcItem, theme::surface_brush());
                    theme::fill(
                        item.hDC,
                        &RECT {
                            right: item.rcItem.left + 3,
                            ..item.rcItem
                        },
                        theme::ACCENT,
                    );
                }
            }
            return 1;
        }
        WM_COMMAND if (201..=205).contains(&(w & 0xffff)) => {
            if data.long.is_some()
                && let Err(e) = finish_long(hwnd, w & 0xffff)
            {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_HOTKEY if data.long.is_some() => {
            if let Some(&(_, _, _, id)) =
                LONG_HOTKEYS.iter().find(|&&(key, _, _, _)| key == w as i32)
                && let Err(e) = finish_long(hwnd, id)
            {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_NCHITTEST if data.long.is_some() => return HTTRANSPARENT as isize,
        WM_MOUSEACTIVATE if data.long.is_some() => return MA_NOACTIVATE as isize,
        WM_COMMAND if (w >> 16) as u32 == LBN_DBLCLK => {
            let index = SendMessageW(data.list, LB_GETCURSEL, 0, 0);
            if let Some(item) = data.history.get(index as usize) {
                let state = &mut *data.app;
                if let Ok(store) = History::new(&state.root, state.settings.history_days)
                    && let Ok(image) = store.load(&item.id, false)
                {
                    let _ = pin(state, image, None, false);
                }
            }
            return 0;
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_NCDESTROY => {
            text_editor::finish(hwnd, data, false);
            if let Some(long) = &data.long {
                KillTimer(hwnd, 3);
                for &id in &long.hotkeys {
                    UnregisterHotKey(hwnd, id);
                }
            }
            let state = &mut *data.app;
            let owned = data.owned;
            state.windows.remove(&(hwnd as usize));
            if !data.font.is_null() {
                DeleteObject(data.font);
            }
            if !data.toolbar_window.is_null() {
                DestroyWindow(data.toolbar_window);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if owned {
                drop(Box::from_raw(ptr));
            }
            PostMessageW(state.hwnd, WM_IDLE, 0, 0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, w, l)
}

fn relative_age(created: u64) -> String {
    let age = ptools_core::now().saturating_sub(created);
    if age < 60 {
        "刚刚".into()
    } else if age < 3600 {
        format!("{}分钟前", age / 60)
    } else if age < 86400 {
        format!("{}小时前", age / 3600)
    } else {
        format!("{}天前", age / 86400)
    }
}
fn parse_color(value: &str) -> Option<Rgba<u8>> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let hex = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            hex.into()
        };
        if hex.len() != 6 {
            return None;
        }
        let rgb = u32::from_str_radix(&hex, 16).ok()?;
        return Some(Rgba([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]));
    }
    let (kind, numbers) = value.split_once('(')?;
    let fields = numbers
        .strip_suffix(')')?
        .split(',')
        .map(|s| s.trim().trim_end_matches('%').parse::<f32>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    if fields.len() != 3 || fields.iter().any(|v| !v.is_finite()) {
        return None;
    }
    if kind == "rgb" {
        if fields.iter().any(|v| !(0.0..=255.0).contains(v)) {
            return None;
        }
        return Some(Rgba([
            fields[0].round() as u8,
            fields[1].round() as u8,
            fields[2].round() as u8,
            255,
        ]));
    }
    if !["hsl", "hsv"].contains(&kind)
        || !(0.0..=360.0).contains(&fields[0])
        || fields[1..].iter().any(|v| !(0.0..=100.0).contains(v))
    {
        return None;
    }
    let h = fields[0].rem_euclid(360.0) / 60.0;
    let s = fields[1] / 100.0;
    let v = fields[2] / 100.0;
    let c = if kind == "hsl" {
        (1.0 - (2.0 * v - 1.0).abs()) * s
    } else {
        v * s
    };
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = if kind == "hsl" { v - c / 2.0 } else { v - c };
    let channels = match h as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    Some(Rgba([
        (255.0 * (channels[0] + m)).round() as u8,
        (255.0 * (channels[1] + m)).round() as u8,
        (255.0 * (channels[2] + m)).round() as u8,
        255,
    ]))
}
fn color_value(pixel: Rgba<u8>, format: &str) -> String {
    let [r, g, b, _] = pixel.0;
    if format == "RGB" {
        return format!("rgb({r}, {g}, {b})");
    }
    if format == "HSL" || format == "HSV" {
        let [r, g, b] = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let l = (max + min) / 2.0;
        let s = if d == 0.0 {
            0.0
        } else if format == "HSV" {
            d / max
        } else {
            d / (1.0 - (2.0 * l - 1.0).abs())
        };
        let v = if format == "HSV" { max } else { l };
        return format!(
            "{}({:.0}, {:.0}%, {:.0}%)",
            format.to_lowercase(),
            h,
            s * 100.0,
            v * 100.0
        );
    }
    format!("#{r:02X}{g:02X}{b:02X}")
}

pub fn vertical_shift(previous: &RgbaImage, next: &RgbaImage) -> Option<u32> {
    if previous.dimensions() != next.dimensions() || previous.height() < 40 {
        return None;
    }
    let w = previous.width();
    let h = previous.height();
    let error = |shift: u32| {
        let mut sum = 0u64;
        let mut count = 0u64;
        for y in (8..h - shift - 8).step_by(8) {
            for x in (w / 10..w - w / 10).step_by(16) {
                let a = previous.get_pixel(x, y + shift);
                let b = next.get_pixel(x, y);
                for c in 0..3 {
                    sum += a[c].abs_diff(b[c]) as u64;
                    count += 1;
                }
            }
        }
        if count < 30 {
            255.0
        } else {
            sum as f64 / count as f64
        }
    };
    if error(0) < 0.5 {
        return Some(0);
    }
    (1..h - 32)
        .map(|shift| (shift, error(shift)))
        .filter(|(_, error)| *error < 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(shift, _)| shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_annotation_changes_exported_pixels_and_keeps_its_own_style() {
        let mut data = empty_view(RgbaImage::from_fn(240, 160, |x, y| {
            Rgba([(x % 240) as u8, (y % 160) as u8, 180, 255])
        }));
        for tool in [
            Tool::Rect,
            Tool::Ellipse,
            Tool::Line,
            Tool::Arrow,
            Tool::Polyline,
            Tool::Pen,
            Tool::Highlight,
            Tool::Text,
            Tool::Number,
        ] {
            data.marks = vec![Mark {
                tool,
                points: vec![[40, 40], [100, 50], [180, 120]],
                text: "标注 2".into(),
                color: toolbar::COLORS[4],
                width: 6,
            }];
            let before = data.image.clone();
            let image = unsafe { rendered(&data) }.unwrap();
            assert_ne!(image, before, "{tool:?}");
            assert_eq!(data.image, before);
            data.color = toolbar::COLORS[0];
            data.stroke = 2;
            assert_eq!(
                unsafe { rendered(&data) }.unwrap(),
                image,
                "existing {tool:?} style changed"
            );
        }
    }
    #[test]
    fn selection_move_keeps_annotations_and_resize_stays_in_bounds() {
        let mut data = empty_view(RgbaImage::new(300, 200));
        data.selection = RECT {
            left: 20,
            top: 30,
            right: 120,
            bottom: 100,
        };
        data.selection_origin = data.selection;
        data.start = [40, 40];
        data.selection_drag = 16;
        data.marks.push(Mark {
            tool: Tool::Arrow,
            points: vec![[40, 50], [80, 80]],
            text: String::new(),
            color: 0,
            width: 3,
        });
        adjust_selection(&mut data, [500, 500]);
        assert_eq!(
            [
                data.selection.left,
                data.selection.top,
                data.selection.right,
                data.selection.bottom
            ],
            [200, 130, 300, 200]
        );
        assert_eq!(data.marks[0].points, vec![[220, 150], [260, 180]]);
        data.selection_origin = data.selection;
        data.selection_drag = 1 | 4;
        adjust_selection(&mut data, [900, 900]);
        assert_eq!([data.selection.left, data.selection.top], [299, 199]);
        assert_eq!(selection_hit(data.selection, [299, 199], 1), 1 | 4);
    }
    #[test]
    fn mosaic_brush_follows_the_path_and_supports_dabs_crop_and_undo() {
        let mut data = empty_view(RgbaImage::from_fn(180, 140, |x, y| {
            Rgba([(x * 17) as u8, (y * 19) as u8, (x * y) as u8, 255])
        }));
        data.screen = Some(RECT {
            left: 0,
            top: 0,
            right: 180,
            bottom: 140,
        });
        data.selection = RECT {
            left: 20,
            top: 20,
            right: 160,
            bottom: 120,
        };
        let original = unsafe { render_image(&data, false) }.unwrap();
        data.marks.push(Mark {
            tool: Tool::Pen,
            points: vec![[30, 30], [130, 30], [130, 100]],
            text: String::new(),
            color: toolbar::MOSAIC_COLOR,
            width: 3,
        });
        let image = unsafe { render_image(&data, false) }.unwrap();
        assert_ne!(*image.get_pixel(40, 10), *original.get_pixel(40, 10));
        assert_eq!(
            *image.get_pixel(40, 60),
            *original.get_pixel(40, 60),
            "mosaic filled the region between the stroke endpoints"
        );
        data.color = toolbar::COLORS[0];
        data.stroke = 6;
        assert_eq!(unsafe { render_image(&data, false) }.unwrap(), image);
        data.redo.push(data.marks.pop().unwrap());
        assert_eq!(unsafe { render_image(&data, false) }.unwrap(), original);
        data.marks.push(data.redo.pop().unwrap());
        assert_eq!(unsafe { render_image(&data, false) }.unwrap(), image);
        data.marks[0].points = vec![[20, 20], [20, 20]];
        let dab = unsafe { render_image(&data, false) }.unwrap();
        assert_ne!(*dab.get_pixel(0, 0), *original.get_pixel(0, 0));
        assert_eq!(
            *dab.get_pixel(17, 0),
            *original.get_pixel(17, 0),
            "round cap exceeded the brush radius"
        );
        assert_eq!(*dab.get_pixel(60, 60), *original.get_pixel(60, 60));
    }
    #[test]
    fn native_events_finish_drag_undo_polyline_and_keep_pin_border_out_of_export() {
        unsafe {
            // Isolate this thread's key state from modifiers held on the real desktop.
            let mut keyboard = [0u8; 256];
            GetKeyboardState(keyboard.as_mut_ptr());
            SetKeyboardState([0u8; 256].as_ptr());
            let mut state = App {
                hwnd: null_mut(),
                root: PathBuf::new(),
                settings: Settings::default(),
                windows: HashSet::new(),
                busy: false,
                delayed: false,
            };
            let mut data = empty_view(RgbaImage::from_pixel(1000, 700, Rgba([255, 255, 255, 255])));
            data.app = &mut state;
            data.screen = Some(RECT {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 700,
            });
            data.selection = RECT {
                left: 100,
                top: 100,
                right: 800,
                bottom: 450,
            };
            let class = wide("ptools.capture.test");
            let module = GetModuleHandleW(null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(view_proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                1000,
                700,
                null_mut(),
                null_mut(),
                module,
                (&mut *data as *mut View).cast(),
            );
            assert!(!hwnd.is_null());
            let at = |x: i32, y: i32| (x | y << 16) as isize;
            data.selection = RECT::default();
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(100, 100));
            SendMessageW(hwnd, WM_LBUTTONUP, 0, at(800, 450));
            assert_eq!(
                crop(&data.image, data.selection).unwrap().dimensions(),
                (700, 350)
            );
            for id in [1, 2, 3, 4, 5, 7, 38, 17] {
                SendMessageW(hwnd, WM_COMMAND, id, 0);
                let count = data.marks.len();
                SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(200, 200));
                // Mouse-up must commit its coordinates even without a move event.
                SendMessageW(hwnd, WM_LBUTTONUP, 0, at(350, 300));
                assert_eq!(data.marks.len(), count + 1);
                if id != 7 {
                    assert_eq!(
                        *data.marks.last().unwrap().points.last().unwrap(),
                        [350, 300]
                    );
                }
                assert!(!data.dragging);
            }
            let previous = data.marks.last().unwrap().points.clone();
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(200, 200));
            SendMessageW(hwnd, WM_COMMAND, 21, 0);
            SendMessageW(hwnd, WM_MOUSEMOVE, 0, at(550, 350));
            assert!(!data.dragging);
            assert_eq!(
                data.marks.last().unwrap().points,
                previous,
                "undo during drag changed the preceding mark"
            );
            SendMessageW(hwnd, WM_COMMAND, 18, 0);
            for [x, y] in [[200, 200], [300, 250], [400, 200]] {
                SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(x, y));
            }
            SendMessageW(hwnd, WM_COMMAND, 21, 0);
            assert!(!data.polyline_active);
            let previous = data.marks.last().unwrap().points.clone();
            SendMessageW(hwnd, WM_MOUSEMOVE, 0, at(500, 350));
            assert_eq!(data.marks.last().unwrap().points, previous);
            SendMessageW(hwnd, WM_COMMAND, 22, 0);
            assert_eq!(
                data.marks.last().unwrap().points,
                vec![[200, 200], [300, 250], [400, 200]]
            );
            // Choosing the mosaic swatch changes tools and commits the polyline preview.
            SendMessageW(hwnd, WM_COMMAND, 18, 0);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(450, 220));
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(520, 280));
            SendMessageW(hwnd, WM_COMMAND, 38, 0);
            assert!(!data.polyline_active);
            assert_eq!(
                data.marks.last().unwrap().points,
                vec![[450, 220], [520, 280]]
            );
            let count = data.marks.len();
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(580, 200));
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, at(610, 260));
            SendMessageW(hwnd, WM_LBUTTONUP, 0, at(650, 220));
            assert_eq!(data.marks.len(), count + 1);
            assert_eq!(data.marks.last().unwrap().tool, Tool::Pen);
            assert_eq!(data.marks.last().unwrap().color, toolbar::MOSAIC_COLOR);
            assert_eq!(
                data.marks.last().unwrap().points,
                vec![[580, 200], [610, 260], [650, 220]]
            );
            SendMessageW(hwnd, WM_LBUTTONDBLCLK, 1, at(400, 200));
            assert_ne!(IsWindow(hwnd), 0, "annotation double click closed the view");
            let evidence = std::env::var_os("PTOOLS_CAPTURE_EVIDENCE").map(PathBuf::from);
            if let Some(root) = &evidence {
                fs::create_dir_all(root).unwrap();
                let mut frame = RgbaImage::new(1000, 700);
                paint_native(&mut frame, |dc| {
                    SendMessageW(hwnd, WM_PRINTCLIENT, dc as usize, 0);
                });
                frame.save(root.join("capture-toolbar.png")).unwrap();
                rendered(&data)
                    .unwrap()
                    .save(root.join("annotations-output.png"))
                    .unwrap();
            }
            // OCR completes in the current screenshot without exporting or replacing it.
            let marks = data.marks.len();
            data.tool = Tool::Rect;
            data.working = true;
            data.recognition_pending = true;
            let recognition = Recognition {
                text: "截图 Hello".into(),
                words: vec![
                    crate::ocr::Word {
                        text: "截图".into(),
                        bounds: [40.0, 40.0, 40.0, 20.0],
                    },
                    crate::ocr::Word {
                        text: "Hello".into(),
                        bounds: [100.0, 40.0, 50.0, 20.0],
                    },
                ],
                language: "zh-Hans-CN".into(),
            };
            let payload = OcrWork {
                window: hwnd as usize,
                token: data.token,
                generation: data.recognition_generation,
                origin: [100, 100],
                translate: false,
                result: Ok((recognition, None)),
            };
            SendMessageW(
                hwnd,
                WM_OCR,
                data.token,
                Box::into_raw(Box::new(payload)) as isize,
            );
            assert_ne!(IsWindow(hwnd), 0);
            assert!(!data.working);
            assert!(data.text_selection.text.is_empty());
            assert_eq!(data.marks.len(), marks);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(141, 141));
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, at(179, 159));
            assert_eq!(data.text_selection.text, "截图");
            assert_eq!(
                data.selection.left, 100,
                "text drag moved the screenshot region"
            );
            assert_eq!(data.marks.len(), marks, "text drag created an annotation");
            assert!(toolbar::enabled(&data, 25));
            // Cancel capture without touching the user's clipboard in a unit test.
            ReleaseCapture();
            SendMessageW(hwnd, WM_CAPTURECHANGED, 0, 0);
            assert!(!data.dragging);
            // The OCR switch coexists with the active rectangle tool.
            SendMessageW(hwnd, WM_COMMAND, 12, 0);
            assert!(!data.text_selection_enabled);
            assert_eq!(data.tool, Tool::Rect);
            assert!(data.text_selection.text.is_empty());
            SendMessageW(hwnd, WM_COMMAND, 12, 0);
            assert!(data.text_selection_enabled);
            assert_eq!(data.tool, Tool::Rect);
            assert!(text_hit(hwnd, &data, [141, 141]));
            data.start = [141, 141];
            select_text(hwnd, &mut data, [179, 159]);
            if let Some(root) = &evidence {
                let mut frame = RgbaImage::new(1000, 700);
                paint_native(&mut frame, |dc| {
                    SendMessageW(hwnd, WM_PRINTCLIENT, dc as usize, 0);
                });
                frame.save(root.join("capture-text-selection.png")).unwrap();
            }
            SendMessageW(hwnd, WM_COMMAND, 0, 0);
            assert!(!contains(toolbar::layout(hwnd, &data).bounds, [120, 120]));
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(120, 120));
            assert!(
                data.text_selection.text.is_empty(),
                "blank drag kept the previous text selection"
            );
            assert!(!toolbar::enabled(&data, 25));
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, at(150, 140));
            SendMessageW(hwnd, WM_LBUTTONUP, 0, at(150, 140));
            assert_eq!(data.selection.left, 130);
            assert_eq!(data.selection.top, 120);
            SendMessageW(hwnd, WM_COMMAND, 1, 0);
            SendMessageW(hwnd, WM_COMMAND, 21, 0);
            assert_eq!(
                data.tool,
                Tool::Rect,
                "undo changed the drawing tool while OCR was enabled"
            );
            assert!(data.text_selection_enabled);
            assert!(data.recognition.is_none() && data.text_selection.text.is_empty());
            // A result from an older crop must not restore obsolete word coordinates.
            let obsolete = OcrWork {
                window: hwnd as usize,
                token: data.token,
                generation: data.recognition_generation.wrapping_sub(1),
                origin: [100, 100],
                translate: false,
                result: Ok((
                    Recognition {
                        text: "旧结果".into(),
                        words: vec![],
                        language: "zh-Hans-CN".into(),
                    },
                    None,
                )),
            };
            SendMessageW(
                hwnd,
                WM_OCR,
                data.token,
                Box::into_raw(Box::new(obsolete)) as isize,
            );
            assert!(data.recognition.is_none());
            SendMessageW(hwnd, WM_COMMAND, 22, 0);
            assert!(data.recognition.is_none());
            state.settings.shadow = true;
            assert_eq!(render_image(&data, false).unwrap().dimensions(), (700, 350));
            assert_eq!(rendered(&data).unwrap().dimensions(), (724, 374));
            // Text is edited by a child control on this image, with no modal form.
            SendMessageW(hwnd, WM_COMMAND, 6, 0);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(240, 270));
            let edit = GetDlgItem(hwnd, 230);
            assert!(!edit.is_null());
            assert_eq!(GetParent(edit), hwnd);
            SetWindowTextW(edit, wide("图内文字 & ptools\r\n第二行").as_ptr());
            let mut editor_rect = RECT::default();
            GetClientRect(edit, &mut editor_rect);
            assert!(
                editor_rect.bottom >= 48,
                "multiline inline preview clipped its second line"
            );
            let count = data.marks.len();
            SendMessageW(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
            assert!(data.text_editor.is_none());
            assert_eq!(data.marks.len(), count + 1);
            let mark = data.marks.last().unwrap();
            assert_eq!(mark.points[0], [240, 270]);
            assert_eq!(mark.text, "图内文字 & ptools\r\n第二行");
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, at(400, 300));
            let edit = GetDlgItem(hwnd, 230);
            SetWindowTextW(edit, wide("取消").as_ptr());
            SendMessageW(edit, EM_SETSEL, 2, 2);
            let mut shift = [0u8; 256];
            shift[VK_SHIFT as usize] = 0x80;
            SetKeyboardState(shift.as_ptr());
            SendMessageW(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
            // A fast chord can queue Shift-up before the translated character.
            SetKeyboardState([0u8; 256].as_ptr());
            SendMessageW(edit, WM_CHAR, VK_RETURN as usize, 0);
            assert!(
                data.text_editor.is_some(),
                "Shift+Enter committed the annotation"
            );
            let mut text = [0u16; 32];
            let len = GetWindowTextW(edit, text.as_mut_ptr(), text.len() as i32);
            assert_eq!(String::from_utf16_lossy(&text[..len as usize]), "取消\r\n");
            SendMessageW(edit, WM_KEYDOWN, VK_ESCAPE as usize, 0);
            assert!(data.text_editor.is_none());
            assert_eq!(data.marks.len(), count + 1);
            assert_ne!(IsWindow(hwnd), 0);
            // A pinned view has a display-only frame and a separate toolbar.
            data.screen = None;
            data.recognition = Some(Recognition {
                text: "截图".into(),
                words: vec![crate::ocr::Word {
                    text: "截图".into(),
                    bounds: [40.0, 40.0, 40.0, 20.0],
                }],
                language: "zh-Hans-CN".into(),
            });
            data.recognition_origin = [0, 0];
            data.start = [142, 142];
            select_text(hwnd, &mut data, [180, 160]);
            assert!(
                data.text_selection.text.is_empty(),
                "pin retained the screenshot crop origin"
            );
            data.start = [42, 42];
            select_text(hwnd, &mut data, [80, 60]);
            assert_eq!(data.text_selection.text, "截图");
            data.marks.clear();
            data.tool = Tool::Select;
            assert_eq!(image_point(hwnd, &data, [2, 2]), [0, 0]);
            assert_eq!(image_point(hwnd, &data, [998, 698]), [1000, 700]);
            let export = rendered(&data).unwrap();
            assert_eq!(export, data.image);
            if let Some(root) = &evidence {
                let mut frame = RgbaImage::new(1000, 700);
                paint_native(&mut frame, |dc| {
                    SendMessageW(hwnd, WM_PRINTCLIENT, dc as usize, 0);
                });
                assert_ne!(*frame.get_pixel(0, 0), *export.get_pixel(0, 0));
                assert_eq!(*frame.get_pixel(2, 2), *export.get_pixel(0, 0));
                frame.save(root.join("pin-frame.png")).unwrap();
            }
            data.editing = true;
            toolbar::sync_pin(hwnd);
            assert!(!data.toolbar_window.is_null());
            SendMessageW(hwnd, WM_COMMAND, 2, 0);
            assert_eq!(data.group_tools[0], 2);
            SendMessageW(hwnd, WM_KEYDOWN, VK_SPACE as usize, 0);
            assert!(!data.editing);
            assert_eq!(data.tool, Tool::Select);
            SendMessageW(hwnd, WM_KEYDOWN, VK_SPACE as usize, 0);
            assert!(data.editing);
            assert_eq!(data.tool, Tool::Ellipse);
            assert_eq!(data.group_tools[0], 2);
            assert!(data.text_selection_enabled);
            if let Some(root) = &evidence {
                let layout = toolbar::layout(data.toolbar_window, &data);
                let mut frame =
                    RgbaImage::new(layout.bounds.right as u32, layout.bounds.bottom as u32);
                paint_native(&mut frame, |dc| {
                    SendMessageW(data.toolbar_window, WM_PRINTCLIENT, dc as usize, 0);
                });
                frame.save(root.join("pin-toolbar.png")).unwrap();
            }
            data.editing = false;
            toolbar::sync_pin(hwnd);
            assert_eq!(IsWindowVisible(data.toolbar_window), 0);
            let bar = data.toolbar_window;
            DestroyWindow(hwnd);
            assert_eq!(
                IsWindow(bar),
                0,
                "floating toolbar leaked after closing pin"
            );
            SetKeyboardState(keyboard.as_ptr());
        }
    }
    #[test]
    fn annotations_render_into_original_pixels_and_highlighter_preserves_contrast() {
        let mut data = empty_view(RgbaImage::from_pixel(160, 100, Rgba([255, 255, 255, 255])));
        data.marks.push(Mark {
            tool: Tool::Line,
            points: vec![[20, 20], [120, 20]],
            text: String::new(),
            color: toolbar::COLORS[0],
            width: 3,
        });
        let image = unsafe { rendered(&data) }.unwrap();
        assert_eq!(*data.image.get_pixel(60, 20), Rgba([255, 255, 255, 255]));
        assert!(image.get_pixel(60, 20)[1] < 100);
        data.marks.clear();
        data.image.put_pixel(60, 50, Rgba([20, 40, 60, 255]));
        data.marks.push(Mark {
            tool: Tool::Highlight,
            points: vec![[20, 50], [120, 50]],
            text: String::new(),
            color: toolbar::COLORS[2],
            width: 3,
        });
        let image = unsafe { rendered(&data) }.unwrap();
        assert!(image.get_pixel(60, 50)[0] < image.get_pixel(80, 50)[0]);
        assert!(image.get_pixel(60, 50)[1] < image.get_pixel(80, 50)[1]);
    }
    #[test]
    fn clipboard_color_formats_interoperate_and_reject_invalid_values() {
        let red = Rgba([255, 0, 0, 255]);
        for format in ["HEX", "RGB", "HSL", "HSV"] {
            assert_eq!(parse_color(&color_value(red, format)), Some(red));
        }
        assert_eq!(parse_color("#abc"), Some(Rgba([170, 187, 204, 255])));
        for bad in [
            "#broken",
            "rgb(256,0,0)",
            "rgb(NaN,0,0)",
            "hsl(0,101%,50%)",
            "text",
        ] {
            assert!(parse_color(bad).is_none());
        }
    }
    #[test]
    fn stitches_overlapping_scroll_and_rejects_unrelated_frames() {
        let image = RgbaImage::from_fn(120, 300, |x, y| {
            Rgba([
                (x * y % 251) as u8,
                (y * 13 % 253) as u8,
                (x + y * 7) as u8,
                255,
            ])
        });
        let previous = image::imageops::crop_imm(&image, 0, 0, 120, 180).to_image();
        let next = image::imageops::crop_imm(&image, 0, 50, 120, 180).to_image();
        assert_eq!(vertical_shift(&previous, &next), Some(50));
        assert_eq!(vertical_shift(&previous, &previous), Some(0));
        assert_eq!(
            vertical_shift(
                &previous,
                &RgbaImage::from_pixel(120, 180, Rgba([255, 255, 255, 255]))
            ),
            None
        );
    }
    #[test]
    fn long_capture_appends_original_pixels_and_keeps_a_fixed_region() {
        let full = RgbaImage::from_fn(180, 480, |x, y| {
            Rgba([
                (x * y % 251) as u8,
                (y * 13 % 253) as u8,
                (x + y * 7) as u8,
                255,
            ])
        });
        let first = image::imageops::crop_imm(&full, 0, 0, 180, 240).to_image();
        let mut long = LongCapture {
            region: RECT {
                left: -600,
                top: 200,
                right: -420,
                bottom: 440,
            },
            previous: first.clone(),
            image: first.clone(),
            paused: false,
            status: String::new(),
            hotkeys: vec![],
        };
        append_long_frame(
            &mut long,
            image::imageops::crop_imm(&full, 0, 80, 180, 240).to_image(),
        );
        assert_eq!(
            long.image,
            image::imageops::crop_imm(&full, 0, 0, 180, 320).to_image()
        );
        assert_eq!(
            [
                long.region.left,
                long.region.top,
                long.region.right,
                long.region.bottom
            ],
            [-600, 200, -420, 440]
        );
        long.paused = true;
        let before = long.image.clone();
        append_long_frame(
            &mut long,
            image::imageops::crop_imm(&full, 0, 160, 180, 240).to_image(),
        );
        assert_eq!(long.image, before);
        long.paused = false;
        append_long_frame(
            &mut long,
            RgbaImage::from_pixel(180, 240, Rgba([255, 255, 255, 255])),
        );
        assert_eq!(long.image, before);
        append_long_frame(
            &mut long,
            image::imageops::crop_imm(&full, 0, 160, 180, 240).to_image(),
        );
        assert_eq!(
            long.image,
            image::imageops::crop_imm(&full, 0, 0, 180, 400).to_image()
        );
    }
    #[test]
    fn long_capture_frame_only_paints_outside_the_capture_rectangle() {
        let mut image = RgbaImage::new(180, 140);
        let selection = RECT {
            left: 30,
            top: 25,
            right: 150,
            bottom: 115,
        };
        unsafe {
            paint_native(&mut image, |dc| {
                draw_long_frame(
                    dc,
                    RECT {
                        left: 0,
                        top: 0,
                        right: 180,
                        bottom: 140,
                    },
                    selection,
                )
            });
        }
        for y in 25..115 {
            for x in 30..150 {
                assert_eq!(
                    &image.get_pixel(x, y).0[..3],
                    &[0, 0, 0],
                    "frame leaked into the captured image"
                );
            }
        }
        for (x, y) in [(29, 25), (30, 24), (150, 25), (30, 115)] {
            assert_ne!(&image.get_pixel(x, y).0[..3], &[0, 0, 0]);
        }
    }
}
