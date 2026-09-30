use crate::{
    history::History,
    interactive::{Invocation, WM_INVOKE, read_invocations},
    native::*,
    ocr::Recognition,
};
use image::{AnimationDecoder, Rgba, RgbaImage};
use ptools_core::{Result, read_json, write_json};
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
mod toolbar;

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
    Line,
    Polyline,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Highlight,
    Text,
    Number,
    Mosaic,
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
    selection_drag: u8,
    selection_origin: RECT,
}
struct LongCapture {
    region: RECT,
    previous: RgbaImage,
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
        WS_POPUP,
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
        selection_drag: 0,
        selection_origin: RECT::default(),
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
        if mark.tool == Tool::Mosaic {
            let block = (mark.width * 4).max(8);
            for y in (r.top.max(0)..r.bottom.min(image.height() as i32)).step_by(block as usize) {
                for x in (r.left.max(0)..r.right.min(image.width() as i32)).step_by(block as usize)
                {
                    let pixel = *image.get_pixel(x as u32, y as u32);
                    for yy in y..(y + block).min(r.bottom).min(image.height() as i32) {
                        for xx in x..(x + block).min(r.right).min(image.width() as i32) {
                            image.put_pixel(xx as u32, yy as u32, pixel);
                        }
                    }
                }
            }
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
                        bottom: 4096,
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
                        DrawTextW(dc, wide(&mark.text).as_ptr(), -1, &mut r, DT_WORDBREAK);
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
        8 => Some(Tool::Mosaic),
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
    if data.working {
        return Err("正在识别，请等待完成".into());
    }
    let image = rendered(data)?;
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
    data.working = true;
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
            if recognition.text.trim().is_empty() {
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
        let ptr = Box::into_raw(Box::new((window, token, result)));
        unsafe {
            if PostMessageW(controller as HWND, WM_OCR, 0, ptr as isize) == 0 {
                drop(Box::from_raw(ptr));
            }
        }
    });
    Ok(())
}

unsafe fn command(hwnd: HWND, id: usize) -> Result<()> {
    let data = &mut *view(hwnd);
    if !toolbar::enabled(data, id) {
        return Ok(());
    }
    if data.dragging {
        data.dragging = false;
        data.selecting_text = false;
        ReleaseCapture();
    }
    if let Some(tool) = selected_tool(id) {
        finish_polyline(data);
        data.tool = tool;
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
        12 | 13 => {
            run_ocr(hwnd, data, id == 13)?;
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
        15 => {
            if let Some(screen) = data.screen {
                let selection = data.selection;
                let region = RECT {
                    left: screen.left + selection.left,
                    top: screen.top + selection.top,
                    right: screen.left + selection.right,
                    bottom: screen.top + selection.bottom,
                };
                let previous = crop(&data.image, selection)?;
                let image = render_image(data, false)?;
                let mut next = empty_view(image.clone());
                next.long = Some(LongCapture { region, previous });
                let width = 500;
                let height = 220;
                let candidates = [
                    [region.right + 8, region.top],
                    [region.left - width - 8, region.top],
                    [region.left, region.bottom + 8],
                    [region.left, region.top - height - 8],
                ];
                let [x, y] = candidates
                    .into_iter()
                    .find(|[x, y]| {
                        *x >= screen.left
                            && *y >= screen.top
                            && *x + width <= screen.right
                            && *y + height <= screen.bottom
                    })
                    .unwrap_or([screen.left, screen.top]);
                let long_hwnd = create_view(
                    state,
                    next,
                    "长截图：滚动目标窗口，Enter结束，Esc取消",
                    RECT {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + height,
                    },
                    false,
                );
                if !long_hwnd.is_null() {
                    child(
                        long_hwnd,
                        "BUTTON",
                        "完成并复制",
                        WS_TABSTOP,
                        201,
                        [12, 174, 130, 34],
                    );
                    child(
                        long_hwnd,
                        "BUTTON",
                        "完成并贴图",
                        WS_TABSTOP,
                        202,
                        [154, 174, 130, 34],
                    );
                    child(
                        long_hwnd,
                        "BUTTON",
                        "完成并保存",
                        WS_TABSTOP,
                        203,
                        [296, 174, 130, 34],
                    );
                    SetTimer(long_hwnd, 3, 500, None);
                    SetForegroundWindow(GetShellWindow());
                }
                DestroyWindow(hwnd);
            }
        }
        16 => {
            settings(state, hwnd)?;
        }
        21 => {
            data.polyline_active = false;
            if let Some(mark) = data.marks.pop() {
                data.redo.push(mark);
            }
        }
        22 => {
            data.polyline_active = false;
            if let Some(mark) = data.redo.pop() {
                data.marks.push(mark);
            }
        }
        23 => toolbar::more(hwnd),
        30..=37 => data.color = toolbar::COLORS[id - 30],
        40..=42 => data.stroke = [2, 3, 6][id - 40],
        100 => {
            DestroyWindow(hwnd);
            return Ok(());
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
    let mut data = empty_view(RgbaImage::from_pixel(720, 420, Rgba([250, 250, 250, 255])));
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
            let work =
                Box::from_raw(l as *mut (usize, usize, Result<(Recognition, Option<String>)>));
            let (window, token, result) = *work;
            if (*state).windows.contains(&window)
                && !view(window as HWND).is_null()
                && (*view(window as HWND)).token == token
            {
                let ptr = Box::into_raw(Box::new(result));
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
                let brush = CreateSolidBrush(0x72675c);
                FrameRect(dc, &client, brush);
                DeleteObject(brush);
                let inner = RECT {
                    left: 1,
                    top: 1,
                    right: client.right - 1,
                    bottom: client.bottom - 1,
                };
                FrameRect(dc, &inner, GetStockObject(WHITE_BRUSH) as HBRUSH);
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
                let pen = CreatePen(PS_SOLID, 2, 0xe09020);
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
                    let brush = CreateSolidBrush(0xe09020);
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
                        FillRect(dc, &r, GetStockObject(WHITE_BRUSH) as HBRUSH);
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
                if data.selection.right <= data.selection.left
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
                    FillRect(dc, &r, GetStockObject(WHITE_BRUSH) as HBRUSH);
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
                        "{}×{}\n{}\nC 复制颜色",
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
            if data.screen.is_none()
                && data.selecting_text
                && let Some(recognition) = &data.recognition
            {
                let selection = rect(data.start, data.point);
                let sx = (client.right - 4) as f32 / data.image.width() as f32;
                let sy = (client.bottom - 4) as f32 / data.image.height() as f32;
                let brush = CreateSolidBrush(0xe09020);
                for word in &recognition.words {
                    let [x, y, width, height] = word.bounds;
                    let r = RECT {
                        left: 2 + (x * sx) as i32,
                        top: 2 + (y * sy) as i32,
                        right: 2 + ((x + width) * sx) as i32,
                        bottom: 2 + ((y + height) * sy) as i32,
                    };
                    let mut overlap = RECT::default();
                    if IntersectRect(&mut overlap, &r, &selection) != 0 {
                        FrameRect(dc, &r, brush);
                    }
                }
                DeleteObject(brush);
            }
            if data.long.is_some() {
                let mut r = RECT {
                    left: 12,
                    top: 12,
                    right: 480,
                    bottom: 55,
                };
                FillRect(dc, &r, GetStockObject(WHITE_BRUSH) as HBRUSH);
                DrawTextW(
                    dc,
                    wide("滚动目标窗口进行拼接，Enter结束，Esc取消").as_ptr(),
                    -1,
                    &mut r,
                    DT_LEFT,
                );
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
            let p = point(l);
            let bar = toolbar::layout(hwnd, data);
            if toolbar::visible(data) && contains(bar.bounds, p) {
                if let Some(id) = bar.hit(p)
                    && let Err(e) = command(hwnd, id)
                {
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
            if data.tool == Tool::Polyline {
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
            if data.screen.is_some() || data.tool != Tool::Select || data.recognition.is_some() {
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
                } else if data.recognition.is_some() && data.tool == Tool::Select {
                    data.selecting_text = true;
                } else {
                    let mut label = String::new();
                    if data.tool == Tool::Text {
                        data.dragging = false;
                        ReleaseCapture();
                        let Some(value) =
                            prompt(hwnd, "标注文字", "", false).filter(|s| !s.trim().is_empty())
                        else {
                            return 0;
                        };
                        label = value;
                    } else if data.tool == Tool::Number {
                        data.dragging = false;
                        ReleaseCapture();
                        label = (data.marks.iter().filter(|m| m.tool == Tool::Number).count() + 1)
                            .to_string();
                    }
                    data.redo.clear();
                    data.marks.push(Mark {
                        tool: data.tool,
                        points: vec![image_point(hwnd, data, p), image_point(hwnd, data, p)],
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
                if data.tool == Tool::Select && data.screen.is_some() {
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
        WM_LBUTTONUP => {
            let p = point(l);
            if data.dragging && data.tool != Tool::Select {
                let mut end = image_point(hwnd, data, p);
                if data.screen.is_some() {
                    end = [
                        end[0].clamp(data.selection.left, data.selection.right - 1),
                        end[1].clamp(data.selection.top, data.selection.bottom - 1),
                    ];
                }
                if let Some(mark) = data.marks.last_mut() {
                    *mark.points.last_mut().unwrap() = end;
                }
            }
            let finish_quick = data.quick && data.dragging && data.tool == Tool::Select;
            if data.dragging
                && data.tool == Tool::Select
                && data.screen.is_some()
                && data.selection_drag != 0
            {
                adjust_selection(data, p);
            }
            if data.dragging
                && data.tool == Tool::Select
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
                if let Some(recognition) = &data.recognition {
                    let mut client = RECT::default();
                    GetClientRect(hwnd, &mut client);
                    let r = rect(data.start, p);
                    let sx = data.image.width() as f32 / (client.right - 4).max(1) as f32;
                    let sy = data.image.height() as f32 / (client.bottom - 4).max(1) as f32;
                    let text = recognition
                        .words
                        .iter()
                        .filter(|word| {
                            let [x, y, width, height] = word.bounds;
                            x + width >= (r.left - 2) as f32 * sx
                                && x <= (r.right - 2) as f32 * sx
                                && y + height >= (r.top - 2) as f32 * sy
                                && y <= (r.bottom - 2) as f32 * sy
                        })
                        .map(|word| word.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let _ = copy_text(hwnd, &crate::ocr::normalize(&text));
                }
                data.selecting_text = false;
            }
            data.dragging = false;
            ReleaseCapture();
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            if data.polyline_active {
                finish_polyline(data);
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
            toolbar::sync_pin(hwnd);
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
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_CONTEXTMENU => {
            let popup = CreatePopupMenu();
            for (id, label) in [
                (9, "复制图片"),
                (10, "另建贴图"),
                (11, "另存为"),
                (12, "识别并选择文字"),
                (13, "翻译"),
                (14, "识别二维码"),
                (1, "矩形标注"),
                (2, "椭圆标注"),
                (3, "箭头标注"),
                (4, "画笔"),
                (5, "荧光笔"),
                (6, "文字标注"),
                (7, "序号"),
                (8, "马赛克"),
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
            if id == 101 {
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
            } else if id == 100 {
                DestroyWindow(hwnd);
            } else if id != 0
                && let Err(e) = command(hwnd, id as usize)
            {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_COMMAND
            if w >> 16 == 0
                && ((0..=23).contains(&(w & 0xffff))
                    || (30..=42).contains(&(w & 0xffff))
                    || w == 100) =>
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
                VK_SPACE if data.screen.is_none() => {
                    finish_polyline(data);
                    data.editing = !data.editing;
                    data.tool = if data.editing {
                        Tool::Rect
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
                0x4d => 8,
                0x4c => 17,
                0x46 => 18,
                0x43 if data.screen.is_some() => {
                    let p = data.point;
                    if p[0] >= 0
                        && p[1] >= 0
                        && p[0] < data.image.width() as i32
                        && p[1] < data.image.height() as i32
                    {
                        let pixel = data.image.get_pixel(p[0] as u32, p[1] as u32);
                        let value = color_value(*pixel, &(*data.app).settings.color_format);
                        let _ = copy_text(hwnd, &value);
                        DestroyWindow(hwnd);
                    }
                    return 0;
                }
                VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN if data.screen.is_some() => {
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
            if data.long.is_some() && w as u16 == VK_RETURN {
                KillTimer(hwnd, 3);
                data.long = None;
                let image = data.image.clone();
                let state = &mut *data.app;
                let _ = store(state, &image, "长截图");
                let _ = copy_image(hwnd, &image);
                DestroyWindow(hwnd);
            } else if let Err(e) = command(hwnd, id) {
                alert(hwnd, &e);
            }
            return 0;
        }
        WM_OCR => {
            data.working = false;
            let result = Box::from_raw(l as *mut Result<(Recognition, Option<String>)>);
            match *result {
                Ok((recognition, translated)) => {
                    if let Some(translated) = translated {
                        let _ = prompt(
                            hwnd,
                            "翻译结果",
                            &format!("原文：\n{}\n\n译文：\n{}", recognition.text, translated),
                            true,
                        );
                    } else {
                        let _ = copy_text(hwnd, &recognition.text);
                        if data.screen.is_some() {
                            let image = rendered(data).unwrap_or_else(|_| data.image.clone());
                            if let Ok(pin) = pin(&mut *data.app, image, None, true) {
                                (*view(pin)).recognition = Some(recognition);
                            }
                            DestroyWindow(hwnd);
                        } else {
                            data.tool = Tool::Select;
                            data.recognition = Some(recognition);
                        }
                    }
                }
                Err(e) => alert(hwnd, &e),
            }
            return 0;
        }
        WM_TIMER if w == 2 => {
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
            let mut own = RECT::default();
            GetWindowRect(hwnd, &mut own);
            let mut intersection = RECT::default();
            let hidden = data
                .long
                .as_ref()
                .is_some_and(|long| IntersectRect(&mut intersection, &own, &long.region) != 0);
            if hidden {
                ShowWindow(hwnd, SW_HIDE);
                windows_sys::Win32::Graphics::Dwm::DwmFlush();
            }
            let next = data
                .long
                .as_ref()
                .and_then(|long| screenshot(long.region).ok());
            if hidden {
                ShowWindow(hwnd, SW_SHOWNA);
            }
            if let Some(long) = &mut data.long
                && let Some(next) = next
                && let Some(shift) = vertical_shift(&long.previous, &next)
            {
                if shift > 0
                    && data.image.height().saturating_add(shift) as u64 * data.image.width() as u64
                        <= 100_000_000
                {
                    let extra = image::imageops::crop_imm(
                        &next,
                        0,
                        next.height() - shift,
                        next.width(),
                        shift,
                    )
                    .to_image();
                    let mut combined =
                        RgbaImage::new(data.image.width(), data.image.height() + shift);
                    image::imageops::overlay(&mut combined, &data.image, 0, 0);
                    image::imageops::overlay(&mut combined, &extra, 0, data.image.height() as i64);
                    data.image = combined;
                }
                long.previous = next;
            }
            InvalidateRect(hwnd, null(), 0);
            return 0;
        }
        WM_MEASUREITEM => {
            (*(l as *mut MEASUREITEMSTRUCT)).itemHeight = 118;
            return 1;
        }
        WM_DRAWITEM => {
            let item = &*(l as *const DRAWITEMSTRUCT);
            if let Some(history) = data.history.get(item.itemID as usize) {
                FillRect(
                    item.hDC,
                    &item.rcItem,
                    GetStockObject(WHITE_BRUSH) as HBRUSH,
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
                SelectObject(item.hDC, data.font);
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
                    FrameRect(
                        item.hDC,
                        &item.rcItem,
                        GetStockObject(BLACK_BRUSH) as HBRUSH,
                    );
                }
            }
            return 1;
        }
        WM_COMMAND if (201..=203).contains(&(w & 0xffff)) => {
            let result = (|| -> Result<bool> {
                match w & 0xffff {
                    203 => {
                        if let Some(path) = save_dialog(hwnd) {
                            save_image(&path, &data.image)?;
                            store(&*data.app, &data.image, "长截图")?;
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    202 => {
                        store(&*data.app, &data.image, "长截图")?;
                        pin(&mut *data.app, data.image.clone(), None, false)?;
                        Ok(true)
                    }
                    _ => {
                        copy_image(hwnd, &data.image)?;
                        store(&*data.app, &data.image, "长截图")?;
                        Ok(true)
                    }
                }
            })();
            match result {
                Ok(true) => {
                    KillTimer(hwnd, 3);
                    DestroyWindow(hwnd);
                }
                Ok(false) => {}
                Err(e) => alert(hwnd, &e),
            }
            return 0;
        }
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
            Tool::Mosaic,
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
    fn native_events_finish_drag_undo_polyline_and_keep_pin_border_out_of_export() {
        unsafe {
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
            for id in [1, 2, 3, 4, 5, 7, 8, 17] {
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
            state.settings.shadow = true;
            assert_eq!(render_image(&data, false).unwrap().dimensions(), (700, 350));
            assert_eq!(rendered(&data).unwrap().dimensions(), (724, 374));
            // A pinned view has a display-only frame and a separate toolbar.
            data.screen = None;
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
}
