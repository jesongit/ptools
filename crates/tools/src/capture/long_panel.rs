use super::*;

const ACTIONS: [usize; 5] = [201, 202, 203, 204, 205];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    RightOutside,
    LeftOutside,
    RightInside,
    LeftInside,
}

/// `bounds` uses screen coordinates; every child rectangle uses panel coordinates.
pub(super) struct Layout {
    pub bounds: RECT,
    pub buttons: Vec<(usize, RECT)>,
    pub preview: RECT,
    pub side: Side,
    status: RECT,
    scale: i32,
}

fn fit_image(image: [u32; 2], width: i32, height: i32) -> [i32; 2] {
    let [iw, ih] = image.map(|n| n.max(1) as i64);
    let width = width.max(1) as i64;
    let height = height.max(1) as i64;
    if width * ih <= height * iw {
        [width as i32, (width * ih / iw).max(1) as i32]
    } else {
        [(height * iw / ih).max(1) as i32, height as i32]
    }
}

impl Layout {
    pub(super) fn new(work: RECT, region: RECT, dpi: u32, image: [u32; 2]) -> Self {
        let work_width = (work.right - work.left).max(1);
        let work_height = (work.bottom - work.top).max(1);
        // Reduce the presentation scale only when the monitor itself cannot hold
        // the complete icon column. The screenshot region never changes.
        let mut scale = (dpi.max(96) as i32)
            .min(work_width * 96 / 68)
            .min(work_height * 96 / 208)
            .max(1);
        loop {
            let s = |v: i32| ((v * scale + 48) / 96).max(1);
            if scale == 1
                || s(4) * 2 + s(36) * 5 + s(20) <= work_height
                    && s(4) * 3 + s(36) + s(16) <= work_width
            {
                break;
            }
            scale -= 1;
        }
        let s = |v: i32| ((v * scale + 48) / 96).max(1);
        let pad = s(4);
        let gap = s(4);
        let cell = s(36);
        let footer = s(20);
        let icon_height = cell * ACTIONS.len() as i32;
        let maximum_height = (work_height - pad * 2 - footer).max(1);
        let maximum_preview_height = s(320).min(maximum_height);
        let desired_preview = fit_image(image, s(96), maximum_preview_height);
        let desired_width = pad * 2 + cell + gap + desired_preview[0];
        let minimum_width = pad * 2 + cell + gap + s(16);

        let sides = [
            Side::RightOutside,
            Side::LeftOutside,
            Side::RightInside,
            Side::LeftInside,
        ];
        let left_at = |side, width| match side {
            Side::RightOutside => region.right + gap,
            Side::LeftOutside => region.left - gap - width,
            Side::RightInside => region.right - gap - width,
            Side::LeftInside => region.left + gap,
        };
        let fits = |side, width| {
            let left = left_at(side, width);
            let inside = !matches!(side, Side::RightInside | Side::LeftInside)
                || left >= region.left + gap && left + width <= region.right - gap;
            inside && left >= work.left && left + width <= work.right
        };
        let capacity = |side| match side {
            Side::RightOutside => work.right - region.right - gap,
            Side::LeftOutside => region.left - gap - work.left,
            Side::RightInside => {
                if region.right - gap > work.right {
                    0
                } else {
                    region.right - gap - (region.left + gap).max(work.left)
                }
            }
            Side::LeftInside => {
                if region.left + gap < work.left {
                    0
                } else {
                    (region.right - gap).min(work.right) - region.left - gap
                }
            }
        };
        let (side, width) = sides
            .iter()
            .copied()
            .find(|&side| fits(side, desired_width))
            .map(|side| (side, desired_width))
            .or_else(|| {
                sides.iter().copied().find_map(|side| {
                    let width = capacity(side).min(desired_width);
                    (width >= minimum_width && fits(side, width)).then_some((side, width))
                })
            })
            .unwrap_or((Side::RightInside, desired_width.min(work_width)));
        let thumbnail = fit_image(
            image,
            (width - pad * 2 - cell - gap).max(1),
            maximum_preview_height,
        );
        let height = (pad * 2 + icon_height.max(thumbnail[1]) + footer).min(work_height);
        let left = left_at(side, width).clamp(work.left, (work.right - width).max(work.left));
        let top = region
            .top
            .clamp(work.top, (work.bottom - height).max(work.top));
        // Keep the icons closest to the selected frame on either side of it.
        let icons_left = match side {
            Side::RightOutside | Side::LeftInside => pad,
            Side::LeftOutside | Side::RightInside => width - pad - cell,
        };
        let preview_left = if icons_left == pad {
            icons_left + cell + gap
        } else {
            icons_left - gap - thumbnail[0]
        };
        Self {
            bounds: RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            },
            buttons: ACTIONS
                .iter()
                .enumerate()
                .map(|(row, &id)| {
                    let top = pad + row as i32 * cell;
                    (
                        id,
                        RECT {
                            left: icons_left,
                            top,
                            right: icons_left + cell,
                            bottom: top + cell,
                        },
                    )
                })
                .collect(),
            preview: RECT {
                left: preview_left,
                top: pad,
                right: preview_left + thumbnail[0],
                bottom: pad + thumbnail[1],
            },
            side,
            status: RECT {
                left: pad,
                top: height - footer - pad,
                right: width - pad,
                bottom: height - pad,
            },
            scale,
        }
    }

    pub(super) fn hit(&self, p: [i32; 2]) -> Option<usize> {
        self.buttons
            .iter()
            .find(|(_, r)| contains(*r, p))
            .map(|(id, _)| *id)
    }
}

struct Panel {
    owner: HWND,
    layout: Layout,
    thumbnail: RgbaImage,
    thumbnail_key: [u32; 4],
    tooltip: HWND,
    tooltip_texts: Vec<Vec<u16>>,
}

unsafe fn layout(owner: HWND, data: &View) -> Layout {
    let long = data.long.as_ref().unwrap();
    let monitor = MonitorFromRect(&long.region, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..zeroed()
    };
    GetMonitorInfoW(monitor, &mut info);
    let mut dpi_x = GetDpiForWindow(owner).max(96);
    let mut dpi_y = dpi_x;
    GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    Layout::new(
        info.rcWork,
        long.region,
        dpi_x,
        [long.image.width(), long.image.height()],
    )
}

fn thumbnail(image: &RgbaImage, layout: &Layout) -> RgbaImage {
    image::imageops::resize(
        image,
        (layout.preview.right - layout.preview.left).max(1) as u32,
        (layout.preview.bottom - layout.preview.top).max(1) as u32,
        image::imageops::FilterType::Triangle,
    )
}

pub(super) unsafe fn sync(owner: HWND) {
    let data = &mut *view(owner);
    if data.long.is_none() {
        return;
    }
    if data.toolbar_window.is_null() {
        let class = wide("ptools.capture.long-panel");
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
            wide("长截图").as_ptr(),
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
    if data.toolbar_window.is_null() {
        return;
    }
    let panel = &mut *(GetWindowLongPtrW(data.toolbar_window, GWLP_USERDATA) as *mut Panel);
    panel.layout = layout(owner, data);
    let image = &data.long.as_ref().unwrap().image;
    let key = [
        image.width(),
        image.height(),
        (panel.layout.preview.right - panel.layout.preview.left) as u32,
        (panel.layout.preview.bottom - panel.layout.preview.top) as u32,
    ];
    if panel.thumbnail_key != key {
        panel.thumbnail = thumbnail(image, &panel.layout);
        panel.thumbnail_key = key;
    }
    sync_tooltips(data.toolbar_window, panel, data.long.as_ref().unwrap());
    let bounds = panel.layout.bounds;
    SetWindowPos(
        data.toolbar_window,
        HWND_TOPMOST,
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        SWP_NOACTIVATE
            | if IsWindowVisible(owner) != 0 {
                SWP_SHOWWINDOW
            } else {
                0
            },
    );
    InvalidateRect(data.toolbar_window, null(), 0);
}

/// Include tooltip bounds so an exterior panel cannot leak its popup into the
/// captured pixels. Sampling explicitly dismisses the popup before hiding.
pub(super) unsafe fn overlaps_capture(owner: HWND, region: RECT) -> bool {
    let hwnd = (*view(owner)).toolbar_window;
    if hwnd.is_null() {
        return false;
    }
    let intersects = |window: HWND| {
        let mut bounds = RECT::default();
        let mut overlap = RECT::default();
        !window.is_null()
            && IsWindowVisible(window) != 0
            && GetWindowRect(window, &mut bounds) != 0
            && IntersectRect(&mut overlap, &bounds, &region) != 0
    };
    if intersects(hwnd) {
        return true;
    }
    let panel = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel;
    !panel.is_null() && intersects((*panel).tooltip)
}

pub(super) unsafe fn dismiss_tooltip(owner: HWND) {
    let hwnd = (*view(owner)).toolbar_window;
    if !hwnd.is_null() {
        let panel = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel;
        if !panel.is_null() && !(*panel).tooltip.is_null() {
            SendMessageW((*panel).tooltip, TTM_POP, 0, 0);
        }
    }
}

fn label(id: usize, paused: bool) -> &'static str {
    match id {
        201 => "复制 · Enter",
        202 => "贴图 · Ctrl+T",
        203 => "保存 · Ctrl+S",
        204 if paused => "继续",
        204 => "暂停",
        205 => "取消 · Esc",
        _ => "",
    }
}

unsafe fn sync_tooltips(hwnd: HWND, panel: &mut Panel, long: &LongCapture) {
    let created = panel.tooltip.is_null();
    if created {
        panel.tooltip = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            TOOLTIPS_CLASSW,
            null(),
            WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            hwnd,
            null_mut(),
            GetModuleHandleW(null()),
            null_mut(),
        );
        if panel.tooltip.is_null() {
            return;
        }
        SetWindowTheme(panel.tooltip, wide("").as_ptr(), wide("").as_ptr());
        SendMessageW(panel.tooltip, TTM_SETTIPBKCOLOR, theme::RAISED as usize, 0);
        SendMessageW(panel.tooltip, TTM_SETTIPTEXTCOLOR, theme::TEXT as usize, 0);
        SendMessageW(
            panel.tooltip,
            TTM_SETMAXTIPWIDTH,
            0,
            240 * panel.layout.scale as isize / 96,
        );
        SendMessageW(panel.tooltip, TTM_SETDELAYTIME, TTDT_INITIAL as usize, 250);
    }
    let descriptions: Vec<_> = ACTIONS
        .iter()
        .map(|&id| wide(label(id, long.paused)))
        .chain(Some(wide(&format!(
            "{}×{} · {}",
            long.image.width(),
            long.image.height(),
            long.status
        ))))
        .collect();
    // Keep the old buffers alive until every native tool has received its new
    // pointer; updating one tool can synchronously repaint another tooltip.
    let _previous_texts = std::mem::replace(&mut panel.tooltip_texts, descriptions);
    for (index, &(id, rect)) in panel
        .layout
        .buttons
        .iter()
        .chain(std::iter::once(&(206, panel.layout.status)))
        .enumerate()
    {
        let tool = TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_SUBCLASS,
            hwnd,
            uId: id,
            rect,
            lpszText: panel.tooltip_texts[index].as_mut_ptr(),
            ..zeroed()
        };
        let ptr = &tool as *const TTTOOLINFOW as isize;
        if created {
            SendMessageW(panel.tooltip, TTM_ADDTOOLW, 0, ptr);
        } else {
            SendMessageW(panel.tooltip, TTM_NEWTOOLRECTW, 0, ptr);
            SendMessageW(panel.tooltip, TTM_UPDATETIPTEXTW, 0, ptr);
        }
    }
}

unsafe fn icon(dc: HDC, id: usize, r: RECT, color: u32, dpi: i32, paused: bool) {
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
        201 => {
            path(&[[8, 3], [21, 3], [21, 17]]);
            Rectangle(dc, x + s(3), y + s(7), x + s(17), y + s(22));
        }
        202 => {
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
        203 => {
            path(&[[4, 3], [18, 3], [22, 7], [22, 22], [4, 22], [4, 3]]);
            path(&[[8, 3], [8, 10], [17, 10], [17, 3]]);
            path(&[[8, 22], [8, 15], [18, 15], [18, 22]]);
        }
        204 if paused => path(&[[7, 4], [21, 12], [7, 21], [7, 4]]),
        204 => {
            path(&[[7, 4], [7, 21]]);
            path(&[[17, 4], [17, 21]]);
        }
        205 => {
            path(&[[6, 6], [19, 19]]);
            path(&[[19, 6], [6, 19]]);
        }
        _ => {}
    }
    SelectObject(dc, old);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
}

unsafe fn draw(dc: HDC, data: &View, layout: &Layout, thumbnail: &RgbaImage) {
    let saved = SaveDC(dc);
    let long = data.long.as_ref().unwrap();
    let width = layout.bounds.right - layout.bounds.left;
    let height = layout.bounds.bottom - layout.bounds.top;
    let bounds = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    theme::fill(dc, &bounds, theme::SURFACE);
    theme::frame(dc, &bounds, theme::BORDER);
    let s = |v: i32| (v * layout.scale + 48) / 96;
    for &(id, r) in &layout.buttons {
        let hovered = data.hover_button == Some(id);
        let primary = id == 201;
        let active = id == 204 && long.paused;
        if primary || hovered || active {
            let inner = RECT {
                left: r.left + s(2),
                top: r.top + s(2),
                right: r.right - s(2),
                bottom: r.bottom - s(2),
            };
            theme::fill(
                dc,
                &inner,
                if primary {
                    theme::ACCENT
                } else if active {
                    theme::SELECTION
                } else {
                    theme::RAISED
                },
            );
        }
        let color = if primary {
            theme::BACKGROUND
        } else if active {
            theme::ACCENT
        } else if id == 205 && hovered {
            theme::DANGER
        } else {
            theme::TEXT
        };
        icon(dc, id, r, color, layout.scale, long.paused);
    }
    draw_pixels(
        dc,
        thumbnail,
        [
            layout.preview.left,
            layout.preview.top,
            layout.preview.right - layout.preview.left,
            layout.preview.bottom - layout.preview.top,
        ],
    );
    theme::frame(dc, &layout.preview, theme::BORDER);
    let mut font_spec: LOGFONTW = zeroed();
    if !data.font.is_null() {
        GetObjectW(
            data.font,
            size_of::<LOGFONTW>() as i32,
            (&mut font_spec as *mut LOGFONTW).cast(),
        );
    } else {
        for (slot, ch) in font_spec
            .lfFaceName
            .iter_mut()
            .zip("Microsoft YaHei UI".encode_utf16())
        {
            *slot = ch;
        }
    }
    font_spec.lfHeight = -s(12).max(1);
    let font = CreateFontIndirectW(&font_spec);
    if !font.is_null() {
        SelectObject(dc, font);
    }
    SetBkMode(dc, TRANSPARENT as i32);
    let warning = !matches!(
        long.status.as_str(),
        "" | "已暂停" | "滚动页面，点击图标完成"
    );
    SetTextColor(
        dc,
        if warning && data.hover_button.is_none() {
            theme::DANGER
        } else {
            theme::MUTED
        },
    );
    let text = if let Some(id) = data.hover_button {
        label(id, long.paused).to_string()
    } else if warning {
        long.status.clone()
    } else {
        format!(
            "{}×{} · {}",
            long.image.width(),
            long.image.height(),
            if long.paused { "暂停" } else { "滚动" }
        )
    };
    let mut status = layout.status;
    DrawTextW(
        dc,
        wide(&text).as_ptr(),
        -1,
        &mut status,
        match layout.side {
            Side::RightOutside | Side::LeftInside => DT_LEFT,
            Side::LeftOutside | Side::RightInside => DT_RIGHT,
        } | DT_SINGLELINE
            | DT_VCENTER
            | DT_END_ELLIPSIS
            | DT_NOPREFIX,
    );
    RestoreDC(dc, saved);
    if !font.is_null() {
        DeleteObject(font);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let owner = (*(l as *const CREATESTRUCTW)).lpCreateParams as HWND;
        let data = &*view(owner);
        let layout = layout(owner, data);
        let panel = Box::new(Panel {
            owner,
            thumbnail: thumbnail(&data.long.as_ref().unwrap().image, &layout),
            thumbnail_key: [0; 4],
            tooltip: null_mut(),
            tooltip_texts: vec![],
            layout,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(panel) as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel;
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !ptr.is_null() {
            let panel = Box::from_raw(ptr);
            if !panel.tooltip.is_null() {
                DestroyWindow(panel.tooltip);
            }
            drop(panel);
        }
        return DefWindowProcW(hwnd, msg, w, l);
    }
    if !ptr.is_null() {
        let panel = &mut *ptr;
        let data_ptr = view(panel.owner);
        if !data_ptr.is_null() && (*data_ptr).long.is_some() {
            let data = &mut *data_ptr;
            match msg {
                WM_MOUSEACTIVATE => return MA_NOACTIVATE as isize,
                WM_ERASEBKGND => return 1,
                WM_PAINT | WM_PRINTCLIENT => {
                    let mut paint: PAINTSTRUCT = zeroed();
                    let dc = if msg == WM_PRINTCLIENT {
                        w as HDC
                    } else {
                        BeginPaint(hwnd, &mut paint)
                    };
                    // The compact panel is buffered so stitching updates do not
                    // flash a blank thumbnail or partially painted icon column.
                    let buffer = CreateCompatibleDC(dc);
                    let bitmap = CreateCompatibleBitmap(
                        dc,
                        panel.layout.bounds.right - panel.layout.bounds.left,
                        panel.layout.bounds.bottom - panel.layout.bounds.top,
                    );
                    if !buffer.is_null() && !bitmap.is_null() {
                        let old = SelectObject(buffer, bitmap);
                        draw(buffer, data, &panel.layout, &panel.thumbnail);
                        BitBlt(
                            dc,
                            0,
                            0,
                            panel.layout.bounds.right - panel.layout.bounds.left,
                            panel.layout.bounds.bottom - panel.layout.bounds.top,
                            buffer,
                            0,
                            0,
                            SRCCOPY,
                        );
                        SelectObject(buffer, old);
                    } else {
                        draw(dc, data, &panel.layout, &panel.thumbnail);
                    }
                    if !bitmap.is_null() {
                        DeleteObject(bitmap);
                    }
                    if !buffer.is_null() {
                        DeleteDC(buffer);
                    }
                    if msg == WM_PAINT {
                        EndPaint(hwnd, &paint);
                    }
                    return 0;
                }
                WM_LBUTTONDOWN => {
                    if let Some(id) = panel.layout.hit(point(l))
                        && let Err(e) = command(panel.owner, id)
                    {
                        alert(panel.owner, &e);
                    }
                    return 0;
                }
                WM_CLOSE => {
                    if let Err(e) = command(panel.owner, 205) {
                        alert(panel.owner, &e);
                    }
                    return 0;
                }
                WM_COMMAND if ACTIONS.contains(&(w & 0xffff)) => {
                    if let Err(e) = command(panel.owner, w & 0xffff) {
                        alert(panel.owner, &e);
                    }
                    return 0;
                }
                WM_MOUSEMOVE => {
                    let hovered = panel.layout.hit(point(l));
                    if data.hover_button != hovered {
                        data.hover_button = hovered;
                        InvalidateRect(hwnd, null(), 0);
                    }
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    return 0;
                }
                WM_MOUSELEAVE => {
                    data.hover_button = None;
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }
                WM_DPICHANGED | WM_DISPLAYCHANGE => {
                    sync(panel.owner);
                    return 0;
                }
                _ => {}
            }
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn assert_inside(layout: &Layout, work: RECT) {
        assert!(layout.bounds.left >= work.left && layout.bounds.top >= work.top);
        assert!(layout.bounds.right <= work.right && layout.bounds.bottom <= work.bottom);
        let width = layout.bounds.right - layout.bounds.left;
        let height = layout.bounds.bottom - layout.bounds.top;
        for &(id, r) in &layout.buttons {
            assert!(r.left >= 0 && r.top >= 0 && r.right <= width && r.bottom <= height);
            assert!(r.bottom <= layout.status.top);
            assert_eq!(
                layout.hit([(r.left + r.right) / 2, (r.top + r.bottom) / 2]),
                Some(id)
            );
        }
        assert!(layout.preview.left >= 0 && layout.preview.right <= width);
        assert!(layout.preview.top >= 0 && layout.preview.bottom <= layout.status.top);
    }

    #[test]
    fn placement_prefers_right_outside_then_left_outside_then_inside_edges() {
        let work = area(0, 0, 1000, 700);
        for (region, side) in [
            (area(200, 100, 650, 600), Side::RightOutside),
            (area(400, 100, 995, 600), Side::LeftOutside),
            (area(5, 100, 995, 600), Side::RightInside),
            (area(5, 100, 1100, 600), Side::LeftInside),
        ] {
            let layout = Layout::new(work, region, 96, [450, 1200]);
            assert_eq!(layout.side, side);
            assert_inside(&layout, work);
            let gap = 4;
            match side {
                Side::RightOutside => assert_eq!(layout.bounds.left, region.right + gap),
                Side::LeftOutside => assert_eq!(layout.bounds.right, region.left - gap),
                Side::RightInside => assert_eq!(layout.bounds.right, region.right - gap),
                Side::LeftInside => assert_eq!(layout.bounds.left, region.left + gap),
            }
        }
    }

    #[test]
    fn keeps_a_vertical_icon_column_and_adjacent_proportional_thumbnail() {
        for dpi in [96, 144, 192, 288] {
            let layout = Layout::new(
                area(0, 0, 1920, 1080),
                area(300, 100, 1200, 900),
                dpi,
                [900, 4200],
            );
            assert_inside(&layout, area(0, 0, 1920, 1080));
            assert_eq!(layout.buttons.len(), ACTIONS.len());
            let column = layout.buttons[0].1.left;
            for (row, &(id, r)) in layout.buttons.iter().enumerate() {
                assert_eq!(id, ACTIONS[row]);
                assert_eq!(r.left, column);
                if row > 0 {
                    assert_eq!(r.top, layout.buttons[row - 1].1.bottom);
                }
            }
            let preview_width = layout.preview.right - layout.preview.left;
            let preview_height = layout.preview.bottom - layout.preview.top;
            assert!((preview_width as f64 / preview_height as f64 - 900.0 / 4200.0).abs() < 0.01);
            let cell = layout.buttons[0].1;
            let separation = if layout.preview.left >= cell.right {
                layout.preview.left - cell.right
            } else {
                cell.left - layout.preview.right
            };
            assert_eq!(separation, (4 * layout.scale + 48) / 96);
        }
    }

    #[test]
    fn negative_monitor_coordinates_taskbar_and_short_regions_do_not_clip_actions() {
        for dpi in [96, 144, 192, 288] {
            for (work, region) in [
                (area(-1920, -200, 0, 840), area(-1918, 780, -10, 1000)),
                (area(-1280, 0, 0, 720), area(-800, 680, -760, 710)),
                (area(0, 0, 320, 600), area(3, 10, 317, 590)),
                (area(0, 0, 160, 240), area(2, 2, 158, 238)),
            ] {
                assert_inside(&Layout::new(work, region, dpi, [700, 4000]), work);
            }
        }
    }

    #[test]
    fn thumbnail_fits_wide_tall_and_maximum_length_images_without_stretching() {
        for image in [
            [700, 350],
            [700, 4000],
            [700, 60000],
            [1, 60000],
            [60000, 1],
        ] {
            let layout = Layout::new(area(0, 0, 1000, 700), area(200, 100, 600, 600), 96, image);
            assert_inside(&layout, area(0, 0, 1000, 700));
            assert!(
                layout.preview.right > layout.preview.left
                    && layout.preview.bottom > layout.preview.top
            );
        }
    }

    #[test]
    fn native_panel_paint_evidence_preserves_icons_preview_and_hover_labels() {
        unsafe {
            let image = RgbaImage::from_fn(320, 1200, |x, y| {
                let line = y % 80 < 3;
                Rgba(if line {
                    [125, 166, 209, 255]
                } else {
                    [245 - (x / 4) as u8, 245 - (y % 80) as u8, 235, 255]
                })
            });
            let mut data = empty_view(image.clone());
            data.long = Some(LongCapture {
                region: area(200, 100, 600, 600),
                previous: image.clone(),
                image,
                paused: false,
                status: String::new(),
                hotkeys: vec![],
            });
            let cases = [
                ("right-outside", area(200, 100, 600, 600)),
                ("left-outside", area(400, 100, 995, 600)),
                ("right-inside", area(5, 100, 995, 600)),
                ("left-inside", area(5, 100, 1100, 600)),
            ];
            for (name, region) in cases {
                let layout = Layout::new(area(0, 0, 1000, 700), region, 96, [320, 1200]);
                let thumbnail = thumbnail(&data.long.as_ref().unwrap().image, &layout);
                let mut frame = RgbaImage::new(
                    (layout.bounds.right - layout.bounds.left) as u32,
                    (layout.bounds.bottom - layout.bounds.top) as u32,
                );
                paint_native(&mut frame, |dc| draw(dc, &data, &layout, &thumbnail));
                let r = layout.buttons[0].1;
                let p = frame.get_pixel((r.left + 3) as u32, (r.top + 3) as u32);
                assert_eq!([p[0], p[1], p[2]], [244, 161, 59]);
                if let Some(root) = std::env::var_os("PTOOLS_CAPTURE_EVIDENCE") {
                    let root = PathBuf::from(root);
                    fs::create_dir_all(&root).unwrap();
                    frame
                        .save(root.join(format!("long-panel-{name}.png")))
                        .unwrap();
                }
            }
            data.long.as_mut().unwrap().paused = true;
            data.hover_button = Some(204);
            let layout = Layout::new(
                area(0, 0, 1000, 700),
                area(200, 100, 600, 600),
                144,
                [320, 1200],
            );
            let thumbnail = thumbnail(&data.long.as_ref().unwrap().image, &layout);
            let mut frame = RgbaImage::new(
                (layout.bounds.right - layout.bounds.left) as u32,
                (layout.bounds.bottom - layout.bounds.top) as u32,
            );
            paint_native(&mut frame, |dc| draw(dc, &data, &layout, &thumbnail));
            assert_eq!(label(204, true), "继续");
            if let Some(root) = std::env::var_os("PTOOLS_CAPTURE_EVIDENCE") {
                frame
                    .save(PathBuf::from(root).join("long-panel-paused-144dpi.png"))
                    .unwrap();
            }
        }
    }
}
