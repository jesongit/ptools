use image::{ImageEncoder, RgbaImage};
use ptools_ui as theme;
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::{
        Controls::{Dialogs::*, *},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
/// # Safety
/// `hwnd` must be null or a live window owned by the calling UI thread.
pub unsafe fn alert(hwnd: HWND, message: &str) {
    MessageBoxW(
        hwnd,
        wide(message).as_ptr(),
        wide("ptools").as_ptr(),
        MB_OK | MB_ICONINFORMATION,
    );
}
/// # Safety
/// `hwnd` must be null or a live window owned by the calling UI thread.
pub unsafe fn confirm(hwnd: HWND, message: &str) -> bool {
    MessageBoxW(
        hwnd,
        wide(message).as_ptr(),
        wide("ptools").as_ptr(),
        MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
    ) == IDYES
}
/// # Safety
/// `hwnd` must identify a live window.
pub unsafe fn text(hwnd: HWND) -> String {
    let mut buffer = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
    let n = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}
/// # Safety
/// `parent` must be a live window on the calling UI thread.
pub unsafe fn child(
    parent: HWND,
    class: &str,
    title: &str,
    style: u32,
    id: usize,
    rect: [i32; 4],
) -> HWND {
    let hwnd = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(title).as_ptr(),
        WS_CHILD
            | WS_VISIBLE
            | style
            | if matches!(class, "BUTTON" | "EDIT" | "LISTBOX" | "SysListView32") {
                WS_TABSTOP
            } else {
                0
            },
        rect[0],
        rect[1],
        rect[2],
        rect[3],
        parent,
        id as HMENU,
        GetModuleHandleW(null()),
        null(),
    );
    static FONT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let font = *FONT.get_or_init(|| {
        CreateFontW(
            -16,
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
        ) as usize
    });
    SendMessageW(hwnd, WM_SETFONT, font, 1);
    theme::window(parent);
    theme::control(hwnd);
    if class == "BUTTON" && style & 0xf == BS_DEFPUSHBUTTON as u32 {
        theme::button(hwnd, theme::PRIMARY_BUTTON);
    }
    hwnd
}
// Clipboard listeners and input methods can briefly hold the clipboard after
// a selection. Give them time to release it without blocking the UI for long.
unsafe fn open_clipboard(hwnd: HWND) -> bool {
    let deadline = Instant::now() + Duration::from_millis(200);
    loop {
        if OpenClipboard(hwnd) != 0 {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

/// # Safety
/// `hwnd` must be null or a live window on the calling thread.
pub unsafe fn copy_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let value = wide(text);
    let memory = GlobalAlloc(GMEM_MOVEABLE, value.len() * 2);
    if memory.is_null() {
        return Err("剪贴板内存分配失败".into());
    }
    let p = GlobalLock(memory);
    if p.is_null() {
        GlobalFree(memory);
        return Err("剪贴板内存锁定失败".into());
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), p.cast(), value.len());
    GlobalUnlock(memory);
    if !open_clipboard(hwnd) {
        GlobalFree(memory);
        return Err("剪贴板正被其他程序占用，请重试".into());
    }
    EmptyClipboard();
    let ok = !SetClipboardData(13, memory).is_null();
    CloseClipboard();
    if ok {
        Ok(())
    } else {
        GlobalFree(memory);
        Err("无法复制文字".into())
    }
}
/// # Safety
/// `hwnd` must be null or a live window on the calling thread.
pub unsafe fn clipboard_text(hwnd: HWND) -> Option<String> {
    if !open_clipboard(hwnd) {
        return None;
    }
    let handle = GetClipboardData(13);
    let result = if handle.is_null() {
        None
    } else {
        let p = GlobalLock(handle);
        if p.is_null() {
            None
        } else {
            let len = GlobalSize(handle) / 2;
            let slice = std::slice::from_raw_parts(p.cast::<u16>(), len);
            let value = String::from_utf16_lossy(
                &slice[..slice.iter().position(|n| *n == 0).unwrap_or(len)],
            );
            GlobalUnlock(handle);
            Some(value)
        }
    };
    CloseClipboard();
    result
}
/// # Safety
/// `dc` must be a valid device context owned by the calling thread.
pub unsafe fn draw_image(dc: HDC, image: &RgbaImage, rect: [i32; 4]) {
    draw_scaled(dc, image, rect, HALFTONE);
}
/// # Safety
/// `dc` must be a valid device context owned by the calling thread.
pub unsafe fn draw_pixels(dc: HDC, image: &RgbaImage, rect: [i32; 4]) {
    draw_scaled(dc, image, rect, COLORONCOLOR);
}
pub fn save_image(path: &std::path::Path, image: &RgbaImage) -> Result<(), String> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        let rgb = image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
            let pixel = image.get_pixel(x, y);
            let alpha = pixel[3] as u16;
            image::Rgb(
                [0, 1, 2].map(|c| ((pixel[c] as u16 * alpha + 255 * (255 - alpha)) / 255) as u8),
            )
        });
        rgb.save(path).map_err(|e| e.to_string())
    } else {
        image.save(path).map_err(|e| e.to_string())
    }
}
unsafe fn draw_scaled(dc: HDC, image: &RgbaImage, rect: [i32; 4], mode: i32) {
    let mut bgra = image.as_raw().clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let mut info: BITMAPINFO = zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = image.width() as i32;
    info.bmiHeader.biHeight = -(image.height() as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    SetStretchBltMode(dc, mode);
    StretchDIBits(
        dc,
        rect[0],
        rect[1],
        rect[2],
        rect[3],
        0,
        0,
        image.width() as i32,
        image.height() as i32,
        bgra.as_ptr().cast(),
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
fn clipboard_image_data(image: &RgbaImage) -> Result<(Vec<u8>, Vec<u8>), String> {
    let width = i32::try_from(image.width()).map_err(|_| "图片过大，无法复制")?;
    let height = i32::try_from(image.height()).map_err(|_| "图片过大，无法复制")?;
    if width == 0 || height == 0 {
        return Err("图片尺寸无效，无法复制".into());
    }
    let image_size = u32::try_from(image.as_raw().len()).map_err(|_| "图片过大，无法复制")?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("图片编码失败：{e}"))?;

    // Chromium 144 (used by QQ NT) rejects negative DIB heights when checking
    // the unsigned image size. Use bottom-up rows with a positive height, and
    // also offer PNG as Chromium does for interoperable transparent images.
    let header = BITMAPV5HEADER {
        bV5Size: size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: width,
        bV5Height: height,
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: image_size,
        bV5RedMask: 0x00ff0000,
        bV5GreenMask: 0x0000ff00,
        bV5BlueMask: 0x000000ff,
        bV5AlphaMask: 0xff000000,
        bV5CSType: 0x73524742,
        ..unsafe { zeroed() }
    };
    let mut dib = vec![0; size_of::<BITMAPV5HEADER>() + image.as_raw().len()];
    unsafe {
        std::ptr::copy_nonoverlapping(
            (&header as *const BITMAPV5HEADER).cast::<u8>(),
            dib.as_mut_ptr(),
            size_of::<BITMAPV5HEADER>(),
        );
    }
    let row_bytes = image.width() as usize * 4;
    for (source, destination) in image.as_raw().chunks_exact(row_bytes).zip(
        dib[size_of::<BITMAPV5HEADER>()..]
            .chunks_exact_mut(row_bytes)
            .rev(),
    ) {
        destination.copy_from_slice(source);
        for pixel in destination.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Ok((png, dib))
}

// Windows takes ownership only after SetClipboardData succeeds. Retain and
// free each allocation automatically on all preparation/publication failures.
struct ClipboardMemory(HGLOBAL);

impl ClipboardMemory {
    unsafe fn new(bytes: &[u8]) -> Result<Self, String> {
        let memory = Self(GlobalAlloc(GMEM_MOVEABLE, bytes.len()));
        if memory.0.is_null() {
            return Err("剪贴板内存分配失败".into());
        }
        let p = GlobalLock(memory.0).cast::<u8>();
        if p.is_null() {
            return Err("剪贴板内存锁定失败".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        GlobalUnlock(memory.0);
        Ok(memory)
    }

    unsafe fn publish(mut self, format: u32) -> Result<(), String> {
        if SetClipboardData(format, self.0).is_null() {
            return Err("无法复制图片".into());
        }
        self.0 = null_mut();
        Ok(())
    }
}

impl Drop for ClipboardMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GlobalFree(self.0) };
        }
    }
}

/// # Safety
/// `hwnd` must identify a live window on the calling thread.
pub unsafe fn copy_image(hwnd: HWND, image: &RgbaImage) -> Result<(), String> {
    copy_image_at(hwnd, image, None)
}

/// # Safety
/// `hwnd` must identify a live window on the calling thread.
pub unsafe fn copy_image_at(
    hwnd: HWND,
    image: &RgbaImage,
    origin: Option<[i32; 2]>,
) -> Result<(), String> {
    let (png_bytes, dib_bytes) = clipboard_image_data(image)?;
    let png_format = RegisterClipboardFormatW(wide("PNG").as_ptr());
    if png_format == 0 {
        return Err("无法注册剪贴板图片格式".into());
    }
    // Prepare both representations before replacing the user's clipboard.
    let png = ClipboardMemory::new(&png_bytes)?;
    let dib = ClipboardMemory::new(&dib_bytes)?;
    let position = if let Some([x, y]) = origin {
        let format = RegisterClipboardFormatW(wide("ptools.capture.origin.v1").as_ptr());
        if format == 0 {
            return Err("无法注册截图位置格式".into());
        }
        let bytes: Vec<u8> = [x, y, image.width() as i32, image.height() as i32]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        Some((format, ClipboardMemory::new(&bytes)?))
    } else {
        None
    };
    if !open_clipboard(hwnd) {
        return Err("剪贴板正被其他程序占用，请重试".into());
    }
    let result = (|| {
        if EmptyClipboard() == 0 {
            return Err("无法清空剪贴板".into());
        }
        // PNG first preserves alpha for applications using format order.
        png.publish(png_format)?;
        dib.publish(17)?; // CF_DIBV5; Windows synthesizes CF_DIB / CF_BITMAP.
        if let Some((format, memory)) = position {
            memory.publish(format)?;
        }
        Ok(())
    })();
    CloseClipboard();
    result
}

/// # Safety
/// Call while the clipboard is open on this thread.
pub unsafe fn clipboard_image_origin(image: &RgbaImage) -> Option<[i32; 2]> {
    let format = RegisterClipboardFormatW(wide("ptools.capture.origin.v1").as_ptr());
    if format == 0 {
        return None;
    }
    let memory = GetClipboardData(format);
    if memory.is_null() || GlobalSize(memory) < 16 {
        return None;
    }
    let ptr = GlobalLock(memory).cast::<u8>();
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, 16);
    let origin = decode_image_origin(bytes, image.dimensions());
    GlobalUnlock(memory);
    origin
}
fn decode_image_origin(bytes: &[u8], size: (u32, u32)) -> Option<[i32; 2]> {
    let bytes: &[u8; 16] = bytes.try_into().ok()?;
    let values: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b))
        .collect();
    (values[2] as u32 == size.0 && values[3] as u32 == size.1).then_some([values[0], values[1]])
}
/// # Safety
/// Call on an initialized desktop UI thread with physical screen coordinates.
pub unsafe fn screenshot(rect: RECT) -> Result<RgbaImage, String> {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 || width as u64 * height as u64 > 100_000_000 {
        return Err("截图区域无效或过大".into());
    }
    let screen = GetDC(null_mut());
    let memory = CreateCompatibleDC(screen);
    let mut info: BITMAPINFO = zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bitmap.is_null() {
        DeleteDC(memory);
        ReleaseDC(null_mut(), screen);
        return Err("无法获取屏幕图像".into());
    }
    let old = SelectObject(memory, bitmap);
    let ok = BitBlt(
        memory,
        0,
        0,
        width,
        height,
        screen,
        rect.left,
        rect.top,
        SRCCOPY | CAPTUREBLT,
    );
    let mut pixels =
        std::slice::from_raw_parts(bits.cast::<u8>(), width as usize * height as usize * 4)
            .to_vec();
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        p[3] = 255;
    }
    SelectObject(memory, old);
    DeleteObject(bitmap);
    DeleteDC(memory);
    ReleaseDC(null_mut(), screen);
    if ok == 0 {
        return Err("屏幕截图失败".into());
    }
    RgbaImage::from_raw(width as u32, height as u32, pixels).ok_or("屏幕数据无效".into())
}
/// # Safety
/// Call on a thread permitted to create GDI objects.
pub unsafe fn text_image(value: &str) -> Result<RgbaImage, String> {
    let dc = CreateCompatibleDC(null_mut());
    let font = CreateFontW(
        -24,
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
        CLEARTYPE_QUALITY as u32,
        0,
        wide("Microsoft YaHei").as_ptr(),
    );
    let old_font = SelectObject(dc, font);
    let mut rect = RECT {
        left: 12,
        top: 12,
        right: 820,
        bottom: 0,
    };
    let value = wide(value);
    DrawTextW(
        dc,
        value.as_ptr(),
        -1,
        &mut rect,
        DT_CALCRECT | DT_WORDBREAK,
    );
    let width = (rect.right + 12).clamp(40, 832);
    let height = (rect.bottom + 12).clamp(40, 12000);
    let mut info: BITMAPINFO = zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bitmap.is_null() {
        SelectObject(dc, old_font);
        DeleteObject(font);
        DeleteDC(dc);
        return Err("无法创建文字图片".into());
    }
    let old_bitmap = SelectObject(dc, bitmap);
    PatBlt(dc, 0, 0, width, height, WHITENESS);
    SetBkMode(dc, TRANSPARENT as i32);
    rect.bottom = height - 12;
    DrawTextW(dc, value.as_ptr(), -1, &mut rect, DT_WORDBREAK);
    let mut pixels =
        std::slice::from_raw_parts(bits.cast::<u8>(), width as usize * height as usize * 4)
            .to_vec();
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        p[3] = 255;
    }
    SelectObject(dc, old_bitmap);
    SelectObject(dc, old_font);
    DeleteObject(bitmap);
    DeleteObject(font);
    DeleteDC(dc);
    RgbaImage::from_raw(width as u32, height as u32, pixels).ok_or("文字图片无效".into())
}
/// # Safety
/// `hwnd` must be null or a live window on the calling UI thread.
pub unsafe fn save_dialog(hwnd: HWND) -> Option<std::path::PathBuf> {
    let mut file = vec![0u16; 32768];
    let filter = wide("PNG 图像\0*.png\0JPEG 图像\0*.jpg\0\0");
    let mut dialog: OPENFILENAMEW = zeroed();
    dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.Flags = OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR;
    if GetSaveFileNameW(&mut dialog) == 0 {
        None
    } else {
        let mut path = std::path::PathBuf::from(String::from_utf16_lossy(
            &file[..file.iter().position(|c| *c == 0).unwrap_or(0)],
        ));
        if path.extension().is_none() {
            path.set_extension(if dialog.nFilterIndex == 2 {
                "jpg"
            } else {
                "png"
            });
            if path.is_file() && !confirm(hwnd, &format!("文件已存在，覆盖 {}？", path.display()))
            {
                return None;
            }
        }
        Some(path)
    }
}

struct Prompt {
    edit: HWND,
    result: Option<String>,
    done: bool,
}

pub struct Field {
    pub label: String,
    pub value: String,
    pub checkbox: bool,
    pub secret: bool,
}
impl Field {
    pub fn text(label: &str, value: impl ToString) -> Self {
        Self {
            label: label.into(),
            value: value.to_string(),
            checkbox: false,
            secret: false,
        }
    }
    pub fn check(label: &str, value: bool) -> Self {
        Self {
            label: label.into(),
            value: value.to_string(),
            checkbox: true,
            secret: false,
        }
    }
    pub fn secret(label: &str, value: &str) -> Self {
        Self {
            secret: true,
            ..Self::text(label, value)
        }
    }
}
struct Form {
    controls: Vec<(HWND, bool)>,
    result: Option<Vec<String>>,
    done: bool,
}
unsafe extern "system" fn form_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (*(l as *const CREATESTRUCTW)).lpCreateParams as isize,
        );
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Form;
    if !state.is_null() {
        if message == WM_COMMAND && w & 0xffff == 1 {
            (*state).result = Some(
                (*state)
                    .controls
                    .iter()
                    .map(|(control, check)| {
                        if *check {
                            (SendMessageW(*control, BM_GETCHECK, 0, 0) == BST_CHECKED as isize)
                                .to_string()
                        } else {
                            text(*control)
                        }
                    })
                    .collect(),
            );
            (*state).done = true;
            DestroyWindow(hwnd);
            return 0;
        }
        if message == WM_CLOSE || (message == WM_COMMAND && w & 0xffff == 2) {
            (*state).done = true;
            DestroyWindow(hwnd);
            return 0;
        }
    }
    DefWindowProcW(hwnd, message, w, l)
}
/// # Safety
/// `parent` must be null or a live window on this UI thread.
pub unsafe fn form(parent: HWND, title: &str, note: &str, fields: &[Field]) -> Option<Vec<String>> {
    let class = wide("ptools.tool.form");
    let module = GetModuleHandleW(null());
    RegisterClassW(&WNDCLASSW {
        lpfnWndProc: Some(form_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: theme::background_brush(),
        ..zeroed()
    });
    let mut state = Form {
        controls: vec![],
        result: None,
        done: false,
    };
    let height = fields.len() as i32 * 34 + 200;
    let hwnd = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class.as_ptr(),
        wide(title).as_ptr(),
        WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        700,
        height,
        parent,
        null_mut(),
        module,
        (&mut state as *mut Form).cast(),
    );
    if hwnd.is_null() {
        return None;
    }
    child(hwnd, "STATIC", note, 0, 3, [16, 12, 652, 60]);
    for (i, field) in fields.iter().enumerate() {
        let y = 76 + i as i32 * 34;
        let control = if field.checkbox {
            let control = child(
                hwnd,
                "BUTTON",
                &field.label,
                BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
                10 + i,
                [16, y, 652, 28],
            );
            SendMessageW(
                control,
                BM_SETCHECK,
                if field.value == "true" {
                    BST_CHECKED as usize
                } else {
                    BST_UNCHECKED as usize
                },
                0,
            );
            control
        } else {
            child(
                hwnd,
                "STATIC",
                &field.label,
                0,
                100 + i,
                [16, y + 4, 278, 26],
            );
            child(
                hwnd,
                "EDIT",
                &field.value,
                WS_BORDER
                    | WS_TABSTOP
                    | ES_AUTOHSCROLL as u32
                    | if field.secret { ES_PASSWORD as u32 } else { 0 },
                10 + i,
                [298, y, 366, 28],
            )
        };
        state.controls.push((control, field.checkbox));
    }
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    child(
        hwnd,
        "BUTTON",
        "确定",
        BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
        1,
        [460, client.bottom - 46, 92, 30],
    );
    child(
        hwnd,
        "BUTTON",
        "取消",
        WS_TABSTOP,
        2,
        [570, client.bottom - 46, 92, 30],
    );
    EnableWindow(parent, 0);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    if let Some((first, _)) = state.controls.first() {
        SetFocus(*first);
    }
    let mut message: MSG = zeroed();
    while !state.done && GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
            SendMessageW(hwnd, WM_CLOSE, 0, 0);
        } else if IsDialogMessageW(hwnd, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    EnableWindow(parent, 1);
    if !parent.is_null() {
        SetForegroundWindow(parent);
    }
    state.result
}

unsafe extern "system" fn prompt_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Prompt;
    if !data.is_null() {
        match message {
            WM_COMMAND if w & 0xffff == 1 => {
                (*data).result = Some(text((*data).edit));
                (*data).done = true;
                DestroyWindow(hwnd);
                return 0;
            }
            WM_COMMAND if w & 0xffff == 2 => {
                (*data).done = true;
                DestroyWindow(hwnd);
                return 0;
            }
            WM_CLOSE => {
                (*data).done = true;
                DestroyWindow(hwnd);
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, message, w, l)
}

/// # Safety
/// `parent` must be null or a live window owned by the calling UI thread.
pub unsafe fn prompt(parent: HWND, title: &str, initial: &str, multiline: bool) -> Option<String> {
    let class = wide("ptools.tool.prompt");
    let module = GetModuleHandleW(null());
    let wc = WNDCLASSW {
        lpfnWndProc: Some(prompt_proc),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: theme::background_brush(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let mut state = Prompt {
        edit: null_mut(),
        result: None,
        done: false,
    };
    let height = if multiline { 350 } else { 160 };
    let hwnd = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class.as_ptr(),
        wide(title).as_ptr(),
        WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        680,
        height,
        parent,
        null_mut(),
        module,
        (&mut state as *mut Prompt).cast(),
    );
    if hwnd.is_null() {
        return None;
    }
    state.edit = child(
        hwnd,
        "EDIT",
        initial,
        WS_BORDER
            | WS_TABSTOP
            | if multiline {
                ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL
            } else {
                ES_AUTOHSCROLL as u32
            },
        10,
        [12, 12, 636, height - 98],
    );
    child(
        hwnd,
        "BUTTON",
        "确定",
        BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
        1,
        [448, height - 77, 92, 30],
    );
    child(
        hwnd,
        "BUTTON",
        "取消",
        WS_TABSTOP,
        2,
        [552, height - 77, 92, 30],
    );
    if !parent.is_null() {
        EnableWindow(parent, 0);
    }
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(state.edit);
    let mut message: MSG = zeroed();
    while !state.done && GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        if message.message == WM_KEYDOWN && message.wParam == 0x1b {
            state.done = true;
            DestroyWindow(hwnd);
            break;
        }
        if IsDialogMessageW(hwnd, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    if !parent.is_null() {
        EnableWindow(parent, 1);
        SetForegroundWindow(parent);
    }
    state.result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_origin_keeps_negative_coordinates_and_rejects_wrong_image_sizes() {
        let bytes: Vec<u8> = [-1800i32, -240, 320, 180]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        assert_eq!(decode_image_origin(&bytes, (320, 180)), Some([-1800, -240]));
        assert_eq!(decode_image_origin(&bytes, (180, 320)), None);
        assert_eq!(decode_image_origin(&bytes[..12], (320, 180)), None);
        assert_eq!(decode_image_origin(&[], (320, 180)), None);
    }

    #[test]
    fn clipboard_png_and_dib_decode_with_matching_orientation_color_and_alpha() {
        let image = RgbaImage::from_raw(
            3,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 17, 33, 65, 255, 20, 40, 60, 64, 255,
                255, 255, 255,
            ],
        )
        .unwrap();
        let (png, dib) = clipboard_image_data(&image).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), image);
        // Independent BMP decoding catches row reversal, BGRA conversion and
        // alpha-mask errors instead of reproducing the clipboard writer.
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(14 + dib.len() as u32).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&(14 + size_of::<BITMAPV5HEADER>() as u32).to_le_bytes());
        bmp.extend_from_slice(&dib);
        assert_eq!(
            image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
                .unwrap()
                .to_rgba8(),
            image
        );
        // The positive height is required by QQ's Chromium bitmap fallback.
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(dib[20..24].try_into().unwrap()), 24);
    }

    #[test]
    fn empty_clipboard_images_are_rejected_before_publication() {
        assert!(clipboard_image_data(&RgbaImage::new(0, 2)).is_err());
        assert!(clipboard_image_data(&RgbaImage::new(2, 0)).is_err());
    }

    #[test]
    fn clipboard_retry_times_out_on_contention_and_acquires_after_release() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || unsafe {
            // Separate window association matters: OpenClipboard(NULL) can
            // reuse this process's already-open clipboard across threads.
            let window = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            assert!(!window.is_null());
            let opened = open_clipboard(window);
            ready_tx.send(opened).unwrap();
            if opened {
                let _ = release_rx.recv();
                std::thread::sleep(Duration::from_millis(30));
                assert_ne!(CloseClipboard(), 0);
            }
            DestroyWindow(window);
        });
        assert!(ready_rx.recv().unwrap());
        let start = Instant::now();
        let blocked = unsafe { open_clipboard(null_mut()) };
        let waited = start.elapsed();
        if blocked {
            unsafe {
                CloseClipboard();
            }
        }
        release_tx.send(()).unwrap();
        let acquired = unsafe { open_clipboard(null_mut()) };
        if acquired {
            unsafe {
                CloseClipboard();
            }
        }
        holder.join().unwrap();
        assert!(!blocked, "another thread still has the clipboard open");
        assert!(waited >= Duration::from_millis(150));
        assert!(acquired, "clipboard should become available after release");
        // Opening and closing alone preserves the user's clipboard contents.
    }

    #[test]
    fn png_preserves_alpha_and_jpeg_flattens_transparency() {
        let root = std::env::temp_dir().join(format!(
            "ptools-export-{}-{}",
            std::process::id(),
            ptools_core::now()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let image = RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 0, 0]));
        save_image(&root.join("output.png"), &image).unwrap();
        save_image(&root.join("output.jpg"), &image).unwrap();
        assert_eq!(
            image::open(root.join("output.png"))
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)[3],
            0
        );
        assert!(
            image::open(root.join("output.jpg"))
                .unwrap()
                .to_rgb8()
                .get_pixel(0, 0)[0]
                > 240
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
