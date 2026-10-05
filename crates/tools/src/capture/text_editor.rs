use super::*;
use windows_sys::Win32::UI::Input::Ime::{
    CPS_CANCEL, CPS_COMPLETE, ImmGetContext, ImmNotifyIME, ImmReleaseContext, NI_COMPOSITIONSTR,
};

pub(super) struct Editor {
    pub(super) hwnd: HWND,
    font: HFONT,
    point: [i32; 2],
    color: u32,
    stroke: i32,
    composing: bool,
    enter_newline: bool,
}

pub(super) unsafe fn begin(hwnd: HWND, data: &mut View, p: [i32; 2]) {
    finish(hwnd, data, true);
    let edit = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        wide("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_BORDER | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
        p[0],
        p[1],
        180,
        36,
        hwnd,
        230 as HMENU,
        GetModuleHandleW(null()),
        null(),
    );
    if edit.is_null() {
        return;
    }
    let point = image_point(hwnd, data, p);
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    let scale = if data.screen.is_some() {
        1.0
    } else {
        (client.bottom - 4) as f32 / data.image.height() as f32
    };
    let font = CreateFontW(
        -(((18 + data.stroke * 2) as f32 * scale).round() as i32).max(8),
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
    SendMessageW(edit, WM_SETFONT, font as usize, 1);
    SendMessageW(edit, EM_SETLIMITTEXT, 32768, 0);
    SendMessageW(
        edit,
        EM_SETMARGINS,
        (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
        0,
    );
    SetWindowSubclass(edit, Some(edit_proc), 230, hwnd as usize);
    data.text_editor = Some(Editor {
        hwnd: edit,
        font,
        point,
        color: data.color,
        stroke: data.stroke,
        composing: false,
        enter_newline: false,
    });
    resize(hwnd, data);
    SetFocus(edit);
}

pub(super) unsafe fn finish(hwnd: HWND, data: &mut View, commit: bool) {
    let Some(editor) = data.text_editor.take() else {
        return;
    };
    // Take the editor out first: completing composition can send EDIT and
    // EN_CHANGE messages back into the owner. Those must not finish it twice.
    if editor.composing && IsWindow(editor.hwnd) != 0 {
        let context = ImmGetContext(editor.hwnd);
        if !context.is_null() {
            ImmNotifyIME(
                context,
                NI_COMPOSITIONSTR,
                if commit { CPS_COMPLETE } else { CPS_CANCEL },
                0,
            );
            ImmReleaseContext(editor.hwnd, context);
        }
    }
    let mut buffer = vec![0u16; GetWindowTextLengthW(editor.hwnd).max(0) as usize + 1];
    let len = GetWindowTextW(editor.hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    let text = String::from_utf16_lossy(&buffer[..len as usize]);
    RemoveWindowSubclass(editor.hwnd, Some(edit_proc), 230);
    DestroyWindow(editor.hwnd);
    DeleteObject(editor.font);
    if commit && !text.trim().is_empty() {
        clear_recognition(data);
        data.marks.push(Mark {
            tool: Tool::Text,
            points: vec![editor.point, editor.point],
            text,
            color: editor.color,
            width: editor.stroke,
        });
        data.redo.clear();
    }
    InvalidateRect(hwnd, null(), 0);
}

pub(super) unsafe fn resize(hwnd: HWND, data: &mut View) {
    let Some(editor) = &mut data.text_editor else {
        return;
    };
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    let (sx, sy, pad) = if data.screen.is_some() {
        (1.0, 1.0, 0)
    } else {
        (
            (client.right - 4) as f32 / data.image.width() as f32,
            (client.bottom - 4) as f32 / data.image.height() as f32,
            2,
        )
    };
    let x = pad + (editor.point[0] as f32 * sx) as i32;
    let y = pad + (editor.point[1] as f32 * sy) as i32;
    let right = if data.screen.is_some() {
        data.selection.right
    } else {
        client.right - 2
    };
    let bottom = if data.screen.is_some() {
        data.selection.bottom
    } else {
        client.bottom - 2
    };
    let max_width = (right - x).max(12);
    let mut spec: LOGFONTW = zeroed();
    GetObjectW(
        editor.font,
        size_of::<LOGFONTW>() as i32,
        (&mut spec as *mut LOGFONTW).cast(),
    );
    let font_height = -(((18 + editor.stroke * 2) as f32 * sy).round() as i32).max(8);
    if spec.lfHeight != font_height {
        spec.lfHeight = font_height;
        let next = CreateFontIndirectW(&spec);
        if !next.is_null() {
            SendMessageW(editor.hwnd, WM_SETFONT, next as usize, 1);
            DeleteObject(editor.font);
            editor.font = next;
        }
    }
    let mut buffer = vec![0u16; GetWindowTextLengthW(editor.hwnd).max(0) as usize + 1];
    GetWindowTextW(editor.hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    let dc = GetDC(editor.hwnd);
    let old = SelectObject(dc, editor.font);
    let mut measured = RECT {
        right: (max_width - 2).max(1),
        ..zeroed()
    };
    DrawTextW(
        dc,
        buffer.as_ptr(),
        -1,
        &mut measured,
        DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
    );
    let mut metrics: TEXTMETRICW = zeroed();
    GetTextMetricsW(dc, &mut metrics);
    SelectObject(dc, old);
    ReleaseDC(editor.hwnd, dc);
    let width = (measured.right + 12).max(180).min(max_width);
    let height = (measured.bottom + 12)
        .max(metrics.tmHeight + 12)
        .min((bottom - y).max(12));
    SetWindowPos(
        editor.hwnd,
        null_mut(),
        x,
        y,
        width,
        height,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

pub(super) unsafe fn colors(dc: HDC, data: &View) -> LRESULT {
    SetTextColor(
        dc,
        data.text_editor.as_ref().map_or(data.color, |e| e.color),
    );
    SetBkColor(dc, theme::SURFACE);
    theme::surface_brush() as isize
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    owner: usize,
) -> LRESULT {
    if msg == WM_SETTEXT {
        // Multiline EDIT does not emit EN_CHANGE for WM_SETTEXT.
        let result = DefSubclassProc(hwnd, msg, w, l);
        let data = view(owner as HWND);
        if !data.is_null() {
            resize(owner as HWND, &mut *data);
        }
        return result;
    }
    let data = view(owner as HWND);
    if !data.is_null()
        && let Some(editor) = &mut (*data).text_editor
    {
        if msg == WM_IME_STARTCOMPOSITION {
            editor.composing = true;
        }
        if msg == WM_IME_ENDCOMPOSITION {
            editor.composing = false;
        }
        if editor.composing && matches!(msg, WM_KEYDOWN | WM_CHAR) {
            return DefSubclassProc(hwnd, msg, w, l);
        }
        if msg == WM_KEYDOWN && w == VK_RETURN as usize {
            // WM_CHAR may be queued after Shift-up; keep the key-down intent.
            editor.enter_newline = GetKeyState(VK_SHIFT as i32) < 0;
            if editor.enter_newline {
                return DefSubclassProc(hwnd, msg, w, l);
            }
        }
        if msg == WM_CHAR && w == VK_RETURN as usize {
            let newline = editor.enter_newline;
            editor.enter_newline = false;
            return if newline {
                DefSubclassProc(hwnd, msg, w, l)
            } else {
                0
            };
        }
    }
    if msg == WM_GETDLGCODE {
        return (DLGC_WANTALLKEYS | DLGC_WANTCHARS) as isize;
    }
    if msg == WM_KEYDOWN && (w == VK_ESCAPE as usize || w == VK_RETURN as usize) {
        SendMessageW(
            owner as HWND,
            WM_COMMIT_TEXT,
            (w != VK_ESCAPE as usize) as usize,
            0,
        );
        SetFocus(owner as HWND);
        return 0;
    }
    if msg == WM_CHAR && (w == VK_ESCAPE as usize || w == VK_RETURN as usize) {
        return 0;
    }
    if msg == WM_KILLFOCUS {
        PostMessageW(owner as HWND, WM_COMMIT_TEXT, 1, hwnd as isize);
    }
    DefSubclassProc(hwnd, msg, w, l)
}
