use super::*;

pub(super) const COLORS: [u32; 8] = [
    0x0048e8, 0x168aff, 0x30dfff, 0x64b92a, 0xeb8b28, 0xb766a6, 0xffffff, 0x252525,
];
const ITEMS: [usize; 18] = [
    0, 1, 2, 17, 18, 3, 4, 5, 6, 7, 8, 21, 22, 10, 11, 23, 100, 9,
];

pub(super) struct Layout {
    pub bounds: RECT,
    pub buttons: Vec<(usize, RECT)>,
    pub status: RECT,
    scale: i32,
}
impl Layout {
    fn new(width: i32, height: i32, dpi: u32, anchor: Option<RECT>) -> Self {
        let s = |v: i32| (v * dpi.max(96) as i32 + 48) / 96;
        let cell = s(34);
        let pad = s(8);
        let columns = ((width - pad * 2) / cell).clamp(1, ITEMS.len() as i32);
        let bar_width = columns * cell + pad * 2;
        let rows = (ITEMS.len() as i32 + columns - 1) / columns;
        let style_columns = ((bar_width - pad * 2) / s(24)).max(1);
        let style_rows = (11 + style_columns - 1) / style_columns;
        let bar_height = pad * 2 + rows * cell + style_rows * s(24) + s(28);
        let left = anchor.map_or(0, |r| {
            (r.right - bar_width).clamp(0, (width - bar_width).max(0))
        });
        let top = anchor.map_or(0, |r| {
            if r.bottom + s(8) + bar_height <= height {
                r.bottom + s(8)
            } else if r.top - s(8) >= bar_height {
                r.top - bar_height - s(8)
            } else {
                (height - bar_height).max(0)
            }
        });
        let mut buttons: Vec<_> = ITEMS
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let x = left + pad + i as i32 % columns * cell;
                let y = top + pad + i as i32 / columns * cell;
                (
                    id,
                    RECT {
                        left: x,
                        top: y,
                        right: x + cell,
                        bottom: y + cell,
                    },
                )
            })
            .collect();
        for (i, id) in (30..=37).chain(40..=42).enumerate() {
            let x = left + pad + i as i32 % style_columns * s(24);
            let y = top + pad + rows * cell + i as i32 / style_columns * s(24);
            buttons.push((
                id,
                RECT {
                    left: x,
                    top: y,
                    right: x + s(24),
                    bottom: y + s(24),
                },
            ));
        }
        Self {
            bounds: RECT {
                left,
                top,
                right: left + bar_width,
                bottom: top + bar_height,
            },
            buttons,
            status: RECT {
                left: left + pad,
                top: top + bar_height - s(28),
                right: left + bar_width - pad,
                bottom: top + bar_height - s(4),
            },
            scale: dpi.max(96) as i32,
        }
    }
    pub fn hit(&self, p: [i32; 2]) -> Option<usize> {
        self.buttons
            .iter()
            .find(|(_, r)| contains(*r, p))
            .map(|(id, _)| *id)
    }
}
pub(super) unsafe fn layout(hwnd: HWND, data: &View) -> Layout {
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    Layout::new(
        client.right,
        client.bottom,
        GetDpiForWindow(hwnd),
        data.screen.map(|_| data.selection),
    )
}
pub(super) fn visible(data: &View) -> bool {
    data.screen.is_some()
        && data.selection.right > data.selection.left
        && data.selection.bottom > data.selection.top
        && !data.dragging
}
pub(super) fn enabled(data: &View, id: usize) -> bool {
    match id {
        21 => !data.marks.is_empty(),
        22 => !data.redo.is_empty(),
        12 | 13 => !data.working && (data.screen.is_none() || visible(data)),
        15 => data.screen.is_some() && visible(data),
        19 | 20 => data.screen.is_some(),
        1..=11 | 14 | 17 | 18 => data.screen.is_none() || visible(data),
        _ => true,
    }
}
fn label(id: usize) -> &'static str {
    match id {
        0 => "框选 / 移动 · V",
        1 => "矩形 · R · 拖动绘制",
        2 => "椭圆 · E · 拖动绘制",
        3 => "箭头 · A · 从起点拖向目标",
        4 => "画笔 · P · 自由绘制",
        5 => "荧光笔 · H · 半透明标记",
        6 => "文字 · T · 单击输入",
        7 => "序号 · N · 单击放置",
        8 => "马赛克 · M · 拖动遮挡",
        9 => "复制并完成 · Enter / Ctrl+C",
        10 => "贴图 · Ctrl+T",
        11 => "另存为 · Ctrl+S",
        12 => "离线识别文字 · Shift+C",
        13 => "翻译 · Ctrl+Q",
        14 => "识别二维码 · Q",
        15 => "长截图",
        16 => "设置",
        17 => "直线 · L",
        18 => "折线 · F · 单击加点，Enter结束",
        19 => "精确尺寸",
        20 => "全部屏幕 · Ctrl+A",
        21 => "撤销 · Ctrl+Z",
        22 => "重做 · Ctrl+Y",
        23 => "更多 · 识别、长截图、尺寸与设置",
        100 => "关闭 / 取消 · Esc",
        30..=37 => "标注颜色 · 只影响下一条标注",
        40..=42 => "粗细 / 文字大小 / 马赛克强度",
        _ => "",
    }
}
unsafe fn fill(dc: HDC, r: RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, &r, brush);
    DeleteObject(brush);
}
pub(super) unsafe fn draw(dc: HDC, data: &View, layout: &Layout) {
    let saved = SaveDC(dc);
    fill(dc, layout.bounds, 0xfafafa);
    let border = CreateSolidBrush(0xcfc7be);
    FrameRect(dc, &layout.bounds, border);
    DeleteObject(border);
    let s = |v: i32| (v * layout.scale + 48) / 96;
    SetBkMode(dc, TRANSPARENT as i32);
    let mut font_spec: LOGFONTW = zeroed();
    GetObjectW(
        data.font,
        size_of::<LOGFONTW>() as i32,
        (&mut font_spec as *mut LOGFONTW).cast(),
    );
    font_spec.lfHeight = -s(15);
    let font = CreateFontIndirectW(&font_spec);
    SelectObject(dc, if font.is_null() { data.font } else { font });
    for &(id, r) in &layout.buttons {
        let active = selected_tool(id) == Some(data.tool)
            || (30..=37).contains(&id) && COLORS[id - 30] == data.color
            || (40..=42).contains(&id) && [2, 3, 6][id - 40] == data.stroke;
        let available = enabled(data, id);
        if active || id == 9 || data.hover_button == Some(id) && available {
            fill(
                dc,
                RECT {
                    left: r.left + s(2),
                    top: r.top + s(2),
                    right: r.right - s(2),
                    bottom: r.bottom - s(2),
                },
                if id == 9 {
                    0xd67c24
                } else if active {
                    0xf8e8d7
                } else {
                    0xeeeae4
                },
            );
        }
        let color = if !available {
            0xc5bbb1
        } else if id == 9 {
            0xffffff
        } else if active {
            0xc56a19
        } else {
            0x514338
        };
        if (30..=37).contains(&id) {
            let pen = CreatePen(PS_SOLID, s(1), if active { 0xc56a19 } else { 0xbebebe });
            let brush = CreateSolidBrush(COLORS[id - 30]);
            let old_pen = SelectObject(dc, pen);
            let old_brush = SelectObject(dc, brush);
            Ellipse(
                dc,
                r.left + s(5),
                r.top + s(5),
                r.right - s(5),
                r.bottom - s(5),
            );
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);
        } else if (40..=42).contains(&id) {
            let radius = [2, 3, 5][id - 40];
            let x = (r.left + r.right) / 2;
            let y = (r.top + r.bottom) / 2;
            let brush = CreateSolidBrush(color);
            let old = SelectObject(dc, brush);
            Ellipse(
                dc,
                x - s(radius),
                y - s(radius),
                x + s(radius),
                y + s(radius),
            );
            SelectObject(dc, old);
            DeleteObject(brush);
        } else {
            icon(dc, id, r, color, layout.scale);
        }
    }
    let id = data.hover_button.unwrap_or_else(|| {
        ITEMS
            .iter()
            .copied()
            .find(|&id| selected_tool(id) == Some(data.tool))
            .unwrap_or(0)
    });
    let text = if data.screen.is_none() && id == 9 {
        "复制图片 · Ctrl+C"
    } else if !enabled(data, id) {
        match id {
            21 => "没有可撤销的标注",
            22 => "没有可重做的标注",
            12 | 13 => "正在识别，请稍候…",
            _ => "请先框选截图区域",
        }
    } else {
        label(id)
    };
    SetTextColor(dc, 0x817568);
    let mut r = layout.status;
    DrawTextW(
        dc,
        wide(text).as_ptr(),
        -1,
        &mut r,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    RestoreDC(dc, saved);
    if !font.is_null() {
        DeleteObject(font);
    }
}
unsafe fn icon(dc: HDC, id: usize, r: RECT, color: u32, dpi: i32) {
    let s = |v: i32| (v * dpi + 48) / 96;
    let x = (r.left + r.right) / 2 - s(12);
    let y = (r.top + r.bottom) / 2 - s(12);
    let pen = CreatePen(PS_SOLID, s(2).max(1), color);
    let old = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    let path = |points: &[[i32; 2]]| {
        MoveToEx(dc, x + s(points[0][0]), y + s(points[0][1]), null_mut());
        for p in &points[1..] {
            LineTo(dc, x + s(p[0]), y + s(p[1]));
        }
    };
    match id {
        0 => {
            path(&[
                [5, 3],
                [5, 20],
                [10, 15],
                [14, 22],
                [17, 20],
                [13, 13],
                [20, 13],
                [5, 3],
            ]);
        }
        1 => {
            Rectangle(dc, x + s(4), y + s(5), x + s(21), y + s(20));
        }
        2 => {
            Ellipse(dc, x + s(3), y + s(4), x + s(22), y + s(21));
        }
        3 => {
            path(&[[4, 20], [20, 4], [11, 4]]);
            path(&[[20, 4], [20, 13]]);
        }
        17 => path(&[[4, 20], [20, 4]]),
        18 => path(&[[3, 17], [8, 6], [15, 18], [22, 5]]),
        4 | 5 => {
            path(&[
                [4, 17],
                [15, 4],
                [20, 9],
                [9, 22],
                [4, 22],
                [4, 17],
                [9, 22],
            ]);
            if id == 5 {
                path(&[[2, 24], [22, 24]]);
            }
        }
        6 => {
            path(&[[4, 5], [20, 5]]);
            path(&[[12, 5], [12, 21]]);
            path(&[[8, 21], [16, 21]]);
        }
        7 => {
            Ellipse(dc, x + s(2), y + s(2), x + s(23), y + s(23));
            path(&[[9, 9], [12, 6], [12, 19]]);
            path(&[[9, 19], [16, 19]]);
        }
        8 => {
            for yy in 0..3 {
                for xx in 0..3 {
                    if (xx + yy) % 2 == 0 {
                        fill(
                            dc,
                            RECT {
                                left: x + s(4 + xx * 6),
                                top: y + s(4 + yy * 6),
                                right: x + s(9 + xx * 6),
                                bottom: y + s(9 + yy * 6),
                            },
                            color,
                        );
                    }
                }
            }
        }
        9 => path(&[[4, 12], [10, 18], [21, 6]]),
        10 => {
            path(&[
                [7, 3],
                [18, 3],
                [16, 10],
                [20, 14],
                [5, 14],
                [9, 10],
                [7, 3],
            ]);
            path(&[[12, 14], [12, 23]]);
        }
        11 => {
            path(&[[4, 3], [18, 3], [22, 7], [22, 22], [4, 22], [4, 3]]);
            path(&[[8, 3], [8, 10], [17, 10], [17, 3]]);
            path(&[[8, 22], [8, 15], [18, 15], [18, 22]]);
        }
        21 => {
            path(&[[9, 4], [3, 10], [9, 16]]);
            path(&[[3, 10], [15, 10], [20, 14], [20, 20]]);
        }
        22 => {
            path(&[[15, 4], [21, 10], [15, 16]]);
            path(&[[21, 10], [9, 10], [4, 14], [4, 20]]);
        }
        23 => {
            for xx in [5, 12, 19] {
                Ellipse(dc, x + s(xx - 1), y + s(11), x + s(xx + 2), y + s(14));
            }
        }
        100 => {
            path(&[[6, 6], [19, 19]]);
            path(&[[19, 6], [6, 19]]);
        }
        _ => {}
    }
    SelectObject(dc, old);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
}
pub(super) unsafe fn more(hwnd: HWND) {
    let data = &*view(hwnd);
    let popup = CreatePopupMenu();
    for id in [12, 13, 14, 15, 19, 20, 16] {
        AppendMenuW(
            popup,
            MF_STRING | if enabled(data, id) { 0 } else { MF_GRAYED },
            id,
            wide(label(id)).as_ptr(),
        );
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
        && let Err(error) = command(hwnd, id as usize)
    {
        alert(hwnd, &error);
    }
}

pub(super) unsafe fn sync_pin(owner: HWND) {
    let data = &mut *view(owner);
    if data.screen.is_some() || data.long.is_some() || !data.list.is_null() {
        return;
    }
    if !data.editing {
        if !data.toolbar_window.is_null() {
            ShowWindow(data.toolbar_window, SW_HIDE);
        }
        return;
    }
    if data.toolbar_window.is_null() {
        let class = wide("ptools.capture.toolbar");
        let module = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..zeroed()
        };
        RegisterClassW(&wc);
        data.toolbar_window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class.as_ptr(),
            wide("贴图标注工具").as_ptr(),
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
    let mut own = RECT::default();
    GetWindowRect(owner, &mut own);
    let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    GetMonitorInfoW(monitor, &mut info);
    let layout = Layout::new(
        info.rcWork.right - info.rcWork.left,
        10000,
        GetDpiForWindow(owner),
        None,
    );
    let width = layout.bounds.right;
    let height = layout.bounds.bottom;
    let x = own.left.clamp(
        info.rcWork.left,
        (info.rcWork.right - width).max(info.rcWork.left),
    );
    let y = if own.bottom + 8 + height <= info.rcWork.bottom {
        own.bottom + 8
    } else {
        (own.top - height - 8).max(info.rcWork.top)
    };
    SetWindowPos(
        data.toolbar_window,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE
            | if IsWindowVisible(owner) != 0 {
                SWP_SHOWWINDOW
            } else {
                0
            },
    );
    InvalidateRect(data.toolbar_window, null(), 0);
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let owner = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND;
    let ptr = if owner.is_null() {
        null_mut()
    } else {
        view(owner)
    };
    if !ptr.is_null() {
        let data = &mut *ptr;
        let layout = layout(hwnd, data);
        match msg {
            WM_MOUSEACTIVATE => return MA_NOACTIVATE as isize,
            WM_PAINT | WM_PRINTCLIENT => {
                let mut paint: PAINTSTRUCT = zeroed();
                let dc = if msg == WM_PRINTCLIENT {
                    w as HDC
                } else {
                    BeginPaint(hwnd, &mut paint)
                };
                draw(dc, data, &layout);
                if msg == WM_PAINT {
                    EndPaint(hwnd, &paint);
                }
                return 0;
            }
            WM_LBUTTONDOWN => {
                if let Some(id) = layout.hit(point(l))
                    && let Err(e) = command(owner, id)
                {
                    alert(owner, &e);
                }
                return 0;
            }
            WM_MOUSEMOVE => {
                data.hover_button = layout.hit(point(l));
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_MOUSELEAVE => {
                data.hover_button = None;
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_layout_keeps_buttons_and_hit_targets_inside_at_multiple_dpi() {
        for dpi in [96, 144, 192] {
            for width in [320, 800, 1920] {
                let layout = Layout::new(
                    width,
                    1080,
                    dpi,
                    Some(RECT {
                        left: 10,
                        top: 100,
                        right: 200,
                        bottom: 1070,
                    }),
                );
                assert!(layout.bounds.right <= width);
                assert!(layout.bounds.bottom <= 1080);
                for &(id, r) in &layout.buttons {
                    assert!(r.right <= layout.bounds.right && r.bottom <= layout.bounds.bottom);
                    assert_eq!(
                        layout.hit([(r.left + r.right) / 2, (r.top + r.bottom) / 2]),
                        Some(id)
                    );
                }
            }
        }
    }
}
