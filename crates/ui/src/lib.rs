//! Shared native theme for the 02 tactical-terminal visual language.
#![allow(unsafe_op_in_unsafe_fn)]

use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::OnceLock,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{Controls::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
pub const BACKGROUND: u32 = rgb(25, 27, 30);
pub const SURFACE: u32 = rgb(35, 38, 43);
pub const RAISED: u32 = rgb(44, 48, 54);
pub const BORDER: u32 = rgb(69, 75, 84);
pub const TEXT: u32 = rgb(240, 238, 232);
pub const MUTED: u32 = rgb(173, 179, 189);
pub const DISABLED: u32 = rgb(119, 126, 137);
pub const ACCENT: u32 = rgb(244, 161, 59);
pub const ACCENT_HOVER: u32 = rgb(255, 183, 94);
pub const SELECTION: u32 = rgb(60, 49, 37);
pub const SUCCESS: u32 = rgb(145, 180, 157);
pub const DANGER: u32 = rgb(241, 143, 131);
pub const SECONDARY_BUTTON: usize = 0;
pub const PRIMARY_BUTTON: usize = 1;
pub const DANGER_BUTTON: usize = 2;
const THEME_SUBCLASS: usize = 0x4e5602;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
// Three shared immutable brushes are deliberately retained for the process lifetime.
fn brushes() -> &'static [usize; 3] {
    static BRUSHES: OnceLock<[usize; 3]> = OnceLock::new();
    BRUSHES.get_or_init(|| unsafe {
        [
            CreateSolidBrush(BACKGROUND) as usize,
            CreateSolidBrush(SURFACE) as usize,
            CreateSolidBrush(SELECTION) as usize,
        ]
    })
}
pub fn background_brush() -> HBRUSH {
    brushes()[0] as HBRUSH
}
pub fn surface_brush() -> HBRUSH {
    brushes()[1] as HBRUSH
}
pub fn selection_brush() -> HBRUSH {
    brushes()[2] as HBRUSH
}

/// # Safety
/// `dc` must be a live device context on this thread.
pub unsafe fn fill(dc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, rect, brush);
    DeleteObject(brush);
}
/// # Safety
/// `dc` must be a live device context on this thread.
pub unsafe fn frame(dc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FrameRect(dc, rect, brush);
    DeleteObject(brush);
}
unsafe fn class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    let n = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

/// Attach the theme without replacing the owner's window procedure or state.
/// # Safety
/// `hwnd` must be a live window owned by the calling thread.
pub unsafe fn window(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let enabled: i32 = 1;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
        (&enabled as *const i32).cast(),
        size_of::<i32>() as u32,
    );
    SetWindowSubclass(hwnd, Some(window_proc), THEME_SUBCLASS, 0);
}

/// Theme a child control while keeping its native input and accessibility behavior.
/// # Safety
/// `hwnd` must be a live control owned by the calling thread.
pub unsafe fn control(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let name = class(hwnd);
    SetWindowTheme(hwnd, wide("DarkMode_Explorer").as_ptr(), null());
    match name.as_str() {
        "Button" | "BUTTON" => button(hwnd, SECONDARY_BUTTON),
        "SysListView32" => {
            SendMessageW(hwnd, LVM_SETBKCOLOR, 0, BACKGROUND as isize);
            SendMessageW(hwnd, LVM_SETTEXTBKCOLOR, 0, BACKGROUND as isize);
            SendMessageW(hwnd, LVM_SETTEXTCOLOR, 0, TEXT as isize);
            SetWindowSubclass(hwnd, Some(window_proc), THEME_SUBCLASS, 0);
        }
        _ => {}
    }
}

/// # Safety
/// `hwnd` must be a live BUTTON control on the calling thread.
pub unsafe fn button(hwnd: HWND, role: usize) {
    SetWindowSubclass(hwnd, Some(button_proc), THEME_SUBCLASS, role);
    InvalidateRect(hwnd, null(), 1);
}

/// Install dark state images after enabling LVS_EX_CHECKBOXES. State indexes and
/// native mouse/keyboard toggling are unchanged.
/// # Safety
/// `hwnd` must be a live list-view control owned by the calling thread.
pub unsafe fn list_checkboxes(hwnd: HWND) {
    let images = ImageList_Create(20, 20, ILC_COLOR24 | ILC_MASK, 2, 0);
    if images == 0 {
        return;
    }
    let screen = GetDC(hwnd);
    let dc = CreateCompatibleDC(screen);
    let bitmap = CreateCompatibleBitmap(screen, 40, 20);
    let previous = SelectObject(dc, bitmap);
    fill(
        dc,
        &RECT {
            left: 0,
            top: 0,
            right: 40,
            bottom: 20,
        },
        BACKGROUND,
    );
    for index in 0..2 {
        let x = index * 20;
        let r = RECT {
            left: x + 2,
            top: 2,
            right: x + 18,
            bottom: 18,
        };
        fill(dc, &r, if index == 1 { ACCENT } else { SURFACE });
        frame(dc, &r, if index == 1 { ACCENT } else { BORDER });
        if index == 1 {
            let pen = CreatePen(PS_SOLID, 2, BACKGROUND);
            let old = SelectObject(dc, pen);
            MoveToEx(dc, x + 5, 10, null_mut());
            LineTo(dc, x + 9, 14);
            LineTo(dc, x + 15, 6);
            SelectObject(dc, old);
            DeleteObject(pen);
        }
    }
    SelectObject(dc, previous);
    let added = ImageList_AddMasked(images, bitmap, BACKGROUND);
    DeleteObject(bitmap);
    DeleteDC(dc);
    ReleaseDC(hwnd, screen);
    if added < 0 {
        ImageList_Destroy(images);
        return;
    }
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, style | LVS_SHAREIMAGELISTS as isize);
    let old = SendMessageW(
        hwnd,
        LVM_SETIMAGELIST,
        LVSIL_STATE as usize,
        images as isize,
    );
    if old != 0 {
        ImageList_Destroy(old as _);
    }
    SetWindowSubclass(hwnd, Some(window_proc), THEME_SUBCLASS, images as usize);
}

unsafe fn paint_selected_row(hwnd: HWND, draw: &NMCUSTOMDRAW) {
    let saved = SaveDC(draw.hdc);
    fill(draw.hdc, &draw.rc, SELECTION);
    SetTextColor(draw.hdc, TEXT);
    SetBkMode(draw.hdc, TRANSPARENT as i32);
    let font = SendMessageW(hwnd, WM_GETFONT, 0, 0) as HFONT;
    if !font.is_null() {
        SelectObject(draw.hdc, font);
    }
    let state = SendMessageW(hwnd, LVM_GETITEMSTATE, draw.dwItemSpec, 0xffff) as u32;
    let images = SendMessageW(hwnd, LVM_GETIMAGELIST, LVSIL_STATE as usize, 0);
    if images != 0 && state >> 12 != 0 {
        ImageList_Draw(
            images as _,
            ((state >> 12) & 0xf) as i32 - 1,
            draw.hdc,
            draw.rc.left + 4,
            draw.rc.top + (draw.rc.bottom - draw.rc.top - 20) / 2,
            ILD_TRANSPARENT,
        );
    }
    let header = SendMessageW(hwnd, LVM_GETHEADER, 0, 0) as HWND;
    let columns = SendMessageW(header, HDM_GETITEMCOUNT, 0, 0).max(0) as usize;
    let mut left = draw.rc.left;
    for order in 0..columns {
        let column = SendMessageW(header, HDM_ORDERTOINDEX, order, 0).max(0) as usize;
        let width = SendMessageW(hwnd, LVM_GETCOLUMNWIDTH, column, 0) as i32;
        let mut buffer = [0u16; 1024];
        let mut item: LVITEMW = zeroed();
        item.iSubItem = column as i32;
        item.pszText = buffer.as_mut_ptr();
        item.cchTextMax = buffer.len() as i32;
        SendMessageW(
            hwnd,
            LVM_GETITEMTEXTW,
            draw.dwItemSpec,
            (&mut item as *mut LVITEMW) as isize,
        );
        let mut rect = RECT {
            left: left + if column == 0 { 30 } else { 6 },
            top: draw.rc.top,
            right: left + width - 6,
            bottom: draw.rc.bottom,
        };
        DrawTextW(
            draw.hdc,
            buffer.as_ptr(),
            -1,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        left += width;
    }
    let marker = RECT {
        right: draw.rc.left + 3,
        ..draw.rc
    };
    fill(draw.hdc, &marker, ACCENT);
    if GetFocus() == hwnd && state & LVIS_FOCUSED != 0 {
        frame(draw.hdc, &draw.rc, ACCENT);
    }
    RestoreDC(draw.hdc, saved);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    owned_images: usize,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            FillRect(w as HDC, &rect, background_brush());
            return 1;
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = w as HDC;
            let input = msg == WM_CTLCOLOREDIT
                || msg == WM_CTLCOLORLISTBOX
                || class(l as HWND).eq_ignore_ascii_case("edit");
            SetTextColor(
                dc,
                if IsWindowEnabled(l as HWND) != 0 {
                    TEXT
                } else {
                    DISABLED
                },
            );
            SetBkColor(dc, if input { SURFACE } else { BACKGROUND });
            return if input {
                surface_brush()
            } else {
                background_brush()
            } as LRESULT;
        }
        WM_NOTIFY if l != 0 => {
            let source = (*(l as *const NMHDR)).hwndFrom;
            let code = (*(l as *const NMHDR)).code;
            if code == NM_CUSTOMDRAW {
                let draw = &mut *(l as *mut NMCUSTOMDRAW);
                let name = class(source);
                if name == "SysListView32" {
                    if draw.dwDrawStage == CDDS_PREPAINT {
                        return CDRF_NOTIFYITEMDRAW as isize;
                    }
                    if draw.dwDrawStage == CDDS_ITEMPREPAINT {
                        // Query the actual list state: custom-draw state flags can include
                        // themed checkbox state and must not tint every row as selected.
                        let selected = SendMessageW(
                            source,
                            LVM_GETITEMSTATE,
                            draw.dwItemSpec,
                            LVIS_SELECTED as isize,
                        ) & LVIS_SELECTED as isize
                            != 0;
                        // The OS theme otherwise overrides clrTextBk with blue selection.
                        // Draw only selected report rows; native state/input stay intact.
                        if selected {
                            paint_selected_row(source, draw);
                            return CDRF_SKIPDEFAULT as isize;
                        }
                        let item = &mut *(l as *mut NMLVCUSTOMDRAW);
                        item.clrText = TEXT;
                        item.clrTextBk = BACKGROUND;
                        return CDRF_NEWFONT as isize;
                    }
                } else if name == "SysHeader32" {
                    if draw.dwDrawStage == CDDS_PREPAINT {
                        return CDRF_NOTIFYITEMDRAW as isize;
                    }
                    if draw.dwDrawStage == CDDS_ITEMPREPAINT {
                        fill(draw.hdc, &draw.rc, SURFACE);
                        frame(draw.hdc, &draw.rc, BORDER);
                        let mut buffer = [0u16; 256];
                        let mut item: HDITEMW = zeroed();
                        item.mask = HDI_TEXT;
                        item.pszText = buffer.as_mut_ptr();
                        item.cchTextMax = buffer.len() as i32;
                        SendMessageW(
                            source,
                            HDM_GETITEMW,
                            draw.dwItemSpec,
                            (&mut item as *mut HDITEMW) as isize,
                        );
                        let mut rect = draw.rc;
                        rect.left += 10;
                        rect.right -= 6;
                        SetTextColor(draw.hdc, MUTED);
                        SetBkMode(draw.hdc, TRANSPARENT as i32);
                        DrawTextW(
                            draw.hdc,
                            buffer.as_ptr(),
                            -1,
                            &mut rect,
                            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                        );
                        return CDRF_SKIPDEFAULT as isize;
                    }
                }
            }
        }
        WM_NCDESTROY => {
            if owned_images != 0 {
                ImageList_Destroy(owned_images as _);
            }
            RemoveWindowSubclass(hwnd, Some(window_proc), id);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, w, l)
}

unsafe fn paint_button(hwnd: HWND, dc: HDC, role: usize) {
    let saved = SaveDC(dc);
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let enabled = IsWindowEnabled(hwnd) != 0;
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor);
    ScreenToClient(hwnd, &mut cursor);
    let hover = enabled && PtInRect(&rect, cursor) != 0;
    let state = SendMessageW(hwnd, BM_GETSTATE, 0, 0) as u32;
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    let check = matches!(style & BS_TYPEMASK as u32, x if x == BS_AUTOCHECKBOX as u32 || x == BS_CHECKBOX as u32);
    let pressed = state & BST_PUSHED != 0;
    let focused = GetFocus() == hwnd;
    let primary = role == PRIMARY_BUTTON && !check && enabled;
    let background = if check {
        BACKGROUND
    } else if primary {
        if hover { ACCENT_HOVER } else { ACCENT }
    } else if pressed {
        SELECTION
    } else if hover {
        RAISED
    } else {
        SURFACE
    };
    fill(dc, &rect, background);
    let ink = if !enabled {
        DISABLED
    } else if primary {
        BACKGROUND
    } else if role == DANGER_BUTTON {
        DANGER
    } else {
        TEXT
    };
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, ink);
    let font = SendMessageW(hwnd, WM_GETFONT, 0, 0) as HFONT;
    if !font.is_null() {
        SelectObject(dc, font);
    }
    let mut label = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
    GetWindowTextW(hwnd, label.as_mut_ptr(), label.len() as i32);
    let mut text_rect = rect;
    text_rect.left += 10;
    text_rect.right -= 10;
    if check {
        let y = (rect.bottom - 16) / 2;
        let box_rect = RECT {
            left: 2,
            top: y,
            right: 18,
            bottom: y + 16,
        };
        let checked = SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
        fill(dc, &box_rect, if checked { ACCENT } else { SURFACE });
        frame(dc, &box_rect, if focused { ACCENT } else { BORDER });
        if checked {
            let pen = CreatePen(PS_SOLID, 2, BACKGROUND);
            let old = SelectObject(dc, pen);
            MoveToEx(dc, 5, y + 8, null_mut());
            LineTo(dc, 9, y + 12);
            LineTo(dc, 15, y + 4);
            SelectObject(dc, old);
            DeleteObject(pen);
        }
        text_rect.left = 28;
    } else {
        frame(dc, &rect, if focused { ACCENT } else { BORDER });
        if primary {
            // Small industrial corner cut, without changing the rectangular hit target.
            let points = [
                POINT {
                    x: rect.right - 7,
                    y: 0,
                },
                POINT {
                    x: rect.right,
                    y: 0,
                },
                POINT {
                    x: rect.right,
                    y: 7,
                },
            ];
            let old_brush = SelectObject(dc, background_brush());
            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            Polygon(dc, points.as_ptr(), 3);
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
        }
    }
    if pressed {
        OffsetRect(&mut text_rect, 1, 1);
    }
    DrawTextW(
        dc,
        label.as_ptr(),
        -1,
        &mut text_rect,
        DT_SINGLELINE
            | DT_VCENTER
            | DT_END_ELLIPSIS
            | DT_NOPREFIX
            | if check { DT_LEFT } else { DT_CENTER },
    );
    if focused {
        let focus = RECT {
            left: 3,
            top: 3,
            right: rect.right - 3,
            bottom: rect.bottom - 3,
        };
        frame(dc, &focus, if primary { BACKGROUND } else { ACCENT });
    }
    RestoreDC(dc, saved);
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    role: usize,
) -> LRESULT {
    match msg {
        WM_PAINT | WM_PRINTCLIENT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = if msg == WM_PAINT {
                BeginPaint(hwnd, &mut ps)
            } else {
                w as HDC
            };
            paint_button(hwnd, dc, role);
            if msg == WM_PAINT {
                EndPaint(hwnd, &ps);
            }
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            InvalidateRect(hwnd, null(), 0);
        }
        WM_MOUSELEAVE | WM_ENABLE | WM_SETFOCUS | WM_KILLFOCUS | BM_SETCHECK | BM_SETSTATE
        | WM_SETTEXT | WM_UPDATEUISTATE => {
            let result = DefSubclassProc(hwnd, msg, w, l);
            InvalidateRect(hwnd, null(), 0);
            return result;
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(button_proc), id);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, w, l)
}
