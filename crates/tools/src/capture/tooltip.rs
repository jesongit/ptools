use super::*;

pub(super) unsafe fn hide(data: &View) {
    if !data.tooltip_window.is_null() && IsWindowVisible(data.tooltip_window) != 0 {
        ShowWindow(data.tooltip_window, SW_HIDE);
    }
}
pub(super) unsafe fn leave(data: &mut View) {
    hide(data);
    data.hover_button = None;
}
pub(super) unsafe fn hover(owner: HWND, source: HWND, id: Option<usize>) {
    let data = &mut *view(owner);
    if data.hover_button != id {
        hide(data);
    }
    data.hover_button = id;
    let mut track = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_HOVER | TME_LEAVE,
        hwndTrack: source,
        dwHoverTime: 350,
    };
    TrackMouseEvent(&mut track);
    if !data.tooltip_window.is_null() && IsWindowVisible(data.tooltip_window) != 0 {
        show(owner, source);
    }
}
pub(super) unsafe fn show(owner: HWND, source: HWND) {
    let data = &mut *view(owner);
    let Some(id) = data.hover_button else {
        hide(data);
        return;
    };
    if data.dragging || data.polyline_active || !data.slider_window.is_null() {
        hide(data);
        return;
    }
    data.tooltip_text = toolbar::hint_text(data, id);
    if data.tooltip_window.is_null() {
        let class = wide("ptools.capture.tooltip");
        let module = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        data.tooltip_window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class.as_ptr(),
            wide(&data.tooltip_text).as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            owner,
            null_mut(),
            module,
            owner.cast(),
        );
    }
    if data.tooltip_window.is_null() {
        return;
    }
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    GetMonitorInfoW(
        MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
        &mut info,
    );
    let dpi = GetDpiForWindow(source).max(96) as i32;
    let s = |v: i32| (v * dpi + 48) / 96;
    let dc = GetDC(data.tooltip_window);
    let saved = SaveDC(dc);
    SelectObject(dc, data.font);
    let mut text = RECT {
        right: s(420).min(info.rcWork.right - info.rcWork.left - s(24)),
        ..zeroed()
    };
    DrawTextW(
        dc,
        wide(&data.tooltip_text).as_ptr(),
        -1,
        &mut text,
        DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
    );
    RestoreDC(dc, saved);
    ReleaseDC(data.tooltip_window, dc);
    let width = text.right + s(16);
    let height = text.bottom + s(12);
    let x = if cursor.x + s(14) + width <= info.rcWork.right {
        cursor.x + s(14)
    } else {
        (cursor.x - width - s(14)).max(info.rcWork.left)
    };
    let y = if cursor.y + s(20) + height <= info.rcWork.bottom {
        cursor.y + s(20)
    } else {
        (cursor.y - height - s(8)).max(info.rcWork.top)
    };
    SetWindowTextW(data.tooltip_window, wide(&data.tooltip_text).as_ptr());
    SetWindowPos(
        data.tooltip_window,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    InvalidateRect(data.tooltip_window, null(), 0);
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let owner = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND;
    if owner.is_null() || view(owner).is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let data = &*view(owner);
    match msg {
        WM_NCHITTEST => return HTTRANSPARENT as isize,
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as isize,
        WM_PAINT | WM_PRINTCLIENT => {
            let mut paint: PAINTSTRUCT = zeroed();
            let dc = if msg == WM_PRINTCLIENT {
                w as HDC
            } else {
                BeginPaint(hwnd, &mut paint)
            };
            let saved = SaveDC(dc);
            let mut r = RECT::default();
            GetClientRect(hwnd, &mut r);
            FillRect(dc, &r, theme::surface_brush());
            let border = CreateSolidBrush(theme::BORDER);
            FrameRect(dc, &r, border);
            DeleteObject(border);
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            InflateRect(&mut r, -(8 * dpi / 96), -(6 * dpi / 96));
            SelectObject(dc, data.font);
            SetTextColor(dc, theme::TEXT);
            SetBkMode(dc, TRANSPARENT as i32);
            DrawTextW(
                dc,
                wide(&data.tooltip_text).as_ptr(),
                -1,
                &mut r,
                DT_WORDBREAK | DT_NOPREFIX,
            );
            RestoreDC(dc, saved);
            if msg == WM_PAINT {
                EndPaint(hwnd, &paint);
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}
