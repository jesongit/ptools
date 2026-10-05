use super::*;
use ptools_ui as theme;

pub(super) const COLORS: [u32; 8] = [
    0x0048e8, 0x168aff, 0x30dfff, 0x64b92a, 0xeb8b28, 0xb766a6, 0xffffff, 0x252525,
];
// The high byte distinguishes the mosaic brush from every valid RGB color.
pub(super) const MOSAIC_COLOR: u32 = 0x01000000;
const ITEMS: [usize; 22] = [
    0, 1, 3, 4, 6, 7, 21, 22, 12, 25, 13, 14, 24, 15, 19, 20, 16, 10, 11, 101, 100, 9,
];
const STYLE_ITEMS: [usize; 12] = [30, 31, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42];
const GROUPS: [&[usize]; 3] = [&[1, 2], &[17, 18, 3], &[4, 5]];
const GROUP_ANCHORS: [usize; 3] = [1, 3, 4];
const MENU_DATA_TAG: usize = 0x5a00_0000;

fn group_index(id: usize) -> Option<usize> {
    GROUP_ANCHORS.iter().position(|&anchor| anchor == id)
}
fn group_choice(choices: &[usize; 3], group: usize) -> usize {
    if GROUPS[group].contains(&choices[group]) {
        choices[group]
    } else {
        GROUP_ANCHORS[group]
    }
}
fn displayed_tool(data: &View, id: usize) -> usize {
    group_index(id).map_or(id, |group| group_choice(&data.group_tools, group))
}
fn active_tool(data: &View, id: usize) -> bool {
    group_index(id).map_or_else(
        || selected_tool(id) == Some(data.tool),
        |group| {
            GROUPS[group]
                .iter()
                .any(|&id| selected_tool(id) == Some(data.tool))
        },
    )
}
pub(super) fn remember_tool(data: &mut View, id: usize) {
    let id = if id == 38 { 4 } else { id };
    if let Some(group) = GROUPS.iter().position(|members| members.contains(&id)) {
        data.group_tools[group] = id;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ButtonHit {
    Command(usize),
    Dropdown(usize),
}

pub(super) struct Layout {
    pub bounds: RECT,
    pub buttons: Vec<(usize, RECT)>,
    pub status: RECT,
    scale: i32,
}
impl Layout {
    fn new(width: i32, height: i32, dpi: u32, anchor: Option<RECT>) -> Self {
        let sizing = |scale: i32| {
            let s = |v: i32| (v * scale + 48) / 96;
            let cell = s(38);
            let pad = s(8);
            let columns = ((width - pad * 2) / cell).clamp(1, ITEMS.len() as i32);
            let bar_width = columns * cell + pad * 2;
            let rows = (ITEMS.len() as i32 + columns - 1) / columns;
            let style_columns = ((bar_width - pad * 2) / s(24)).max(1);
            let style_rows = (STYLE_ITEMS.len() as i32 + style_columns - 1) / style_columns;
            let bar_height = pad * 2 + rows * cell + style_rows * s(24) + s(28);
            (
                cell,
                pad,
                columns,
                bar_width,
                rows,
                style_columns,
                bar_height,
            )
        };
        let mut scale = dpi.max(96) as i32;
        let (cell, pad, columns, bar_width, rows, style_columns, bar_height) = loop {
            let size = sizing(scale);
            if scale <= 96 || size.3 <= width && size.6 <= height {
                break size;
            }
            // Keep every action visible on a small work area at large display scales.
            scale -= 1;
        };
        let s = |v: i32| (v * scale + 48) / 96;
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
        for (i, &id) in STYLE_ITEMS.iter().enumerate() {
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
            scale,
        }
    }
    pub fn hit(&self, p: [i32; 2]) -> Option<usize> {
        self.buttons
            .iter()
            .find(|(_, r)| contains(*r, p))
            .map(|(id, _)| *id)
    }
    fn arrow_rect(&self, r: RECT) -> RECT {
        RECT {
            left: r.right - (11 * self.scale + 48) / 96,
            ..r
        }
    }
    fn split_hit(&self, p: [i32; 2], choices: &[usize; 3]) -> Option<ButtonHit> {
        let &(id, r) = self.buttons.iter().find(|(_, r)| contains(*r, p))?;
        if let Some(group) = group_index(id) {
            if contains(self.arrow_rect(r), p) {
                Some(ButtonHit::Dropdown(group))
            } else {
                Some(ButtonHit::Command(group_choice(choices, group)))
            }
        } else {
            Some(ButtonHit::Command(id))
        }
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
        && data.long.is_none()
        && data.selection.right > data.selection.left
        && data.selection.bottom > data.selection.top
        && !data.dragging
}
pub(super) fn enabled(data: &View, id: usize) -> bool {
    if data.working && matches!(id, 0..=8 | 15 | 17..=22 | 24 | 30..=42) {
        return false;
    }
    match id {
        21 => !data.marks.is_empty(),
        22 => !data.redo.is_empty(),
        12 => true,
        13 => !data.recognition_pending && (data.screen.is_none() || visible(data)),
        15 => data.screen.is_some() && visible(data),
        19 | 20 => data.screen.is_some(),
        25 => data.text_selection_enabled && !data.text_selection.text.is_empty(),
        101 => data.file.is_some(),
        1..=11 | 14 | 17 | 18 | 24 => data.screen.is_none() || visible(data),
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
        6 => "文字 · T · 图内输入，Enter完成，Shift+Enter换行",
        7 => "序号 · N · 单击放置",
        9 => "复制图片并完成 · Enter / Ctrl+C",
        10 => "贴图 · Ctrl+T",
        11 => "另存为 · Ctrl+S",
        12 => "文字框选开关 · Shift+C · 文字上拖选，空白处移动或绘制",
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
        24 => "取色 · C · 单击图片复制颜色",
        25 => "复制所选文字 · Ctrl+C / Enter",
        100 => "关闭 / 取消 · Esc",
        101 => "打开原文件",
        30..=37 => "标注颜色 · 只影响下一条标注",
        38 => "马赛克画笔 · M · 沿路径遮挡",
        40..=42 => "画笔粗细 / 文字大小",
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
    fill(dc, layout.bounds, theme::SURFACE);
    let border = CreateSolidBrush(theme::BORDER);
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
        let active = active_tool(data, id)
            || id == 12 && data.text_selection_enabled
            || (30..=37).contains(&id) && COLORS[id - 30] == data.color
            || id == 38 && data.tool == Tool::Pen && data.color == MOSAIC_COLOR
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
                    theme::ACCENT
                } else if active {
                    theme::SELECTION
                } else {
                    theme::RAISED
                },
            );
        }
        let color = if !available {
            theme::DISABLED
        } else if id == 9 {
            theme::BACKGROUND
        } else if active {
            theme::ACCENT
        } else {
            theme::TEXT
        };
        if (30..=37).contains(&id) {
            let pen = CreatePen(
                PS_SOLID,
                s(1),
                if active { theme::ACCENT } else { theme::BORDER },
            );
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
        } else if id == 38 {
            mosaic_swatch(dc, r, active, layout.scale);
        } else if id == 12 {
            text_switch(dc, r, data.text_selection_enabled, layout.scale);
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
            if group_index(id).is_some() {
                let arrow = layout.arrow_rect(r);
                icon(
                    dc,
                    displayed_tool(data, id),
                    RECT {
                        right: arrow.left,
                        ..r
                    },
                    color,
                    layout.scale,
                );
                dropdown_arrow(dc, arrow, color, layout.scale);
            } else {
                icon(dc, id, r, color, layout.scale);
            }
        }
    }
    let id = data.hover_button.unwrap_or_else(|| {
        ITEMS
            .iter()
            .copied()
            .find(|&id| active_tool(data, id))
            .unwrap_or(0)
    });
    let id = displayed_tool(data, id);
    let text = if id == 12 {
        if !data.text_selection_enabled {
            "文字框选已关闭 · Shift+C开启"
        } else if data.recognition_pending {
            "文字框选已开启 · 后台识别中…"
        } else if data.recognition_error.is_some() {
            "文字识别不可用 · 关闭后重新开启可重试"
        } else {
            "文字框选已开启 · 文字上拖选，空白处移动或绘制"
        }
    } else if id == 9 && !data.text_selection.text.is_empty() {
        if data.screen.is_none() {
            "复制图片"
        } else {
            "复制图片并完成"
        }
    } else if data.screen.is_none() && id == 9 {
        "复制图片 · Ctrl+C"
    } else if !enabled(data, id) {
        match id {
            _ if data.working => "正在识别，请稍候…",
            21 => "没有可撤销的标注",
            22 => "没有可重做的标注",
            25 => "请先在图片上选择文字",
            101 => "当前图片没有原文件",
            _ => "请先框选截图区域",
        }
    } else {
        label(id)
    };
    SetTextColor(dc, theme::MUTED);
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
unsafe fn dropdown_arrow(dc: HDC, r: RECT, color: u32, dpi: i32) {
    let s = |v: i32| (v * dpi + 48) / 96;
    let x = (r.left + r.right) / 2;
    let y = (r.top + r.bottom) / 2;
    let pen = CreatePen(PS_SOLID, s(1).max(1), color);
    let brush = CreateSolidBrush(color);
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, brush);
    let points = [
        POINT {
            x: x - s(3),
            y: y - s(1),
        },
        POINT {
            x: x + s(3),
            y: y - s(1),
        },
        POINT { x, y: y + s(2) },
    ];
    Polygon(dc, points.as_ptr(), points.len() as i32);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
}
unsafe fn text_switch(dc: HDC, r: RECT, checked: bool, dpi: i32) {
    let s = |v: i32| (v * dpi + 48) / 96;
    let x = (r.left + r.right) / 2;
    let y = (r.top + r.bottom) / 2;
    let pen = CreatePen(
        PS_SOLID,
        s(1).max(1),
        if checked { theme::ACCENT } else { theme::MUTED },
    );
    let brush = CreateSolidBrush(if checked {
        theme::ACCENT
    } else {
        theme::BORDER
    });
    let old_pen = SelectObject(dc, pen);
    let old_brush = SelectObject(dc, brush);
    RoundRect(dc, x - s(12), y - s(6), x + s(12), y + s(6), s(12), s(12));
    let thumb = CreateSolidBrush(theme::TEXT);
    SelectObject(dc, thumb);
    let cx = x + if checked { s(6) } else { -s(6) };
    Ellipse(dc, cx - s(4), y - s(4), cx + s(4), y + s(4));
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    DeleteObject(thumb);
    DeleteObject(brush);
    DeleteObject(pen);
}
unsafe fn mosaic_swatch(dc: HDC, r: RECT, active: bool, dpi: i32) {
    let s = |v: i32| (v * dpi + 48) / 96;
    let r = RECT {
        left: r.left + s(5),
        top: r.top + s(5),
        right: r.right - s(5),
        bottom: r.bottom - s(5),
    };
    for row in 0..3 {
        for column in 0..3 {
            fill(
                dc,
                RECT {
                    left: r.left + (r.right - r.left) * column / 3,
                    top: r.top + (r.bottom - r.top) * row / 3,
                    right: r.left + (r.right - r.left) * (column + 1) / 3,
                    bottom: r.top + (r.bottom - r.top) * (row + 1) / 3,
                },
                if (row + column) % 2 == 0 {
                    theme::MUTED
                } else {
                    theme::RAISED
                },
            );
        }
    }
    let border = CreateSolidBrush(if active { theme::ACCENT } else { theme::BORDER });
    FrameRect(dc, &r, border);
    DeleteObject(border);
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
        12 => {
            path(&[[3, 8], [3, 3], [8, 3]]);
            path(&[[16, 3], [21, 3], [21, 8]]);
            path(&[[3, 16], [3, 21], [8, 21]]);
            path(&[[16, 21], [21, 21], [21, 16]]);
            path(&[[7, 8], [17, 8]]);
            path(&[[12, 8], [12, 17]]);
            path(&[[9, 17], [15, 17]]);
        }
        13 => {
            path(&[[2, 6], [14, 6]]);
            path(&[[8, 3], [8, 6]]);
            path(&[[12, 6], [9, 11], [3, 15]]);
            path(&[[5, 9], [10, 13]]);
            path(&[[12, 21], [17, 10], [22, 21]]);
            path(&[[14, 17], [20, 17]]);
        }
        14 => {
            for [xx, yy] in [[3, 3], [15, 3], [3, 15]] {
                Rectangle(dc, x + s(xx), y + s(yy), x + s(xx + 6), y + s(yy + 6));
            }
            path(&[[15, 15], [18, 15], [18, 21], [21, 21], [21, 17]]);
        }
        15 => {
            Rectangle(dc, x + s(4), y + s(2), x + s(20), y + s(23));
            path(&[[8, 7], [16, 7]]);
            path(&[[12, 11], [12, 19]]);
            path(&[[8, 15], [12, 19], [16, 15]]);
        }
        16 => {
            path(&[
                [9, 3],
                [15, 3],
                [15, 6],
                [18, 8],
                [21, 7],
                [23, 12],
                [20, 14],
                [20, 17],
                [22, 20],
                [17, 23],
                [15, 20],
                [11, 20],
                [9, 23],
                [4, 20],
                [6, 17],
                [6, 14],
                [3, 12],
                [5, 7],
                [8, 8],
                [9, 6],
                [9, 3],
            ]);
            Ellipse(dc, x + s(9), y + s(9), x + s(17), y + s(17));
        }
        19 => {
            Rectangle(dc, x + s(7), y + s(7), x + s(21), y + s(20));
            path(&[[7, 2], [7, 4], [21, 4], [21, 2]]);
            path(&[[2, 7], [4, 7], [4, 20], [2, 20]]);
        }
        20 => {
            Rectangle(dc, x + s(3), y + s(5), x + s(16), y + s(16));
            path(&[[18, 8], [22, 8], [22, 19], [9, 19]]);
            path(&[[9, 16], [9, 22]]);
            path(&[[5, 22], [18, 22]]);
        }
        21 => {
            path(&[[9, 4], [3, 10], [9, 16]]);
            path(&[[3, 10], [15, 10], [20, 14], [20, 20]]);
        }
        22 => {
            path(&[[15, 4], [21, 10], [15, 16]]);
            path(&[[21, 10], [9, 10], [4, 14], [4, 20]]);
        }
        24 => {
            path(&[[7, 14], [15, 6], [19, 10], [11, 18], [6, 20], [7, 14]]);
            path(&[[13, 4], [21, 12]]);
            path(&[[16, 3], [19, 3], [22, 6], [22, 9]]);
        }
        25 => {
            path(&[[8, 4], [18, 4], [18, 18]]);
            Rectangle(dc, x + s(4), y + s(8), x + s(15), y + s(22));
            path(&[[7, 12], [12, 12]]);
            path(&[[9, 12], [9, 18]]);
        }
        100 => {
            path(&[[6, 6], [19, 19]]);
            path(&[[19, 6], [6, 19]]);
        }
        101 => {
            path(&[[11, 4], [4, 4], [4, 21], [21, 21], [21, 14]]);
            path(&[[13, 3], [22, 3], [22, 12]]);
            path(&[[11, 14], [22, 3]]);
        }
        _ => {}
    }
    SelectObject(dc, old);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
}
fn menu_text(id: usize) -> Option<(&'static str, &'static str, &'static str)> {
    match id {
        1 => Some(("矩形", "拖动绘制矩形", "R")),
        2 => Some(("椭圆", "拖动绘制椭圆", "E")),
        17 => Some(("直线", "从起点拖向终点", "L")),
        18 => Some(("折线", "单击加点，Enter结束", "F")),
        3 => Some(("箭头", "从起点拖向目标", "A")),
        4 => Some(("画笔", "自由绘制", "P")),
        5 => Some(("荧光笔", "半透明标记", "H")),
        _ => None,
    }
}
fn menu_scale(item_data: usize, id: usize) -> Option<i32> {
    if item_data & 0xff00_0000 != MENU_DATA_TAG || item_data & 0xff != id || menu_text(id).is_none()
    {
        return None;
    }
    let scale = ((item_data >> 8) & 0xffff) as i32;
    (scale >= 96).then_some(scale)
}
pub(super) fn measure_menu(item: &mut MEASUREITEMSTRUCT) -> bool {
    if item.CtlType != ODT_MENU {
        return false;
    }
    let Some(scale) = menu_scale(item.itemData, item.itemID as usize) else {
        return false;
    };
    item.itemWidth = ((260 * scale + 48) / 96) as u32;
    item.itemHeight = ((52 * scale + 48) / 96) as u32;
    true
}
unsafe fn menu_font(data: &View, height: i32) -> HFONT {
    let mut spec: LOGFONTW = zeroed();
    GetObjectW(
        data.font,
        size_of::<LOGFONTW>() as i32,
        (&mut spec as *mut LOGFONTW).cast(),
    );
    spec.lfHeight = -height;
    CreateFontIndirectW(&spec)
}
pub(super) unsafe fn draw_menu(data: &View, item: &DRAWITEMSTRUCT) -> bool {
    if item.CtlType != ODT_MENU {
        return false;
    }
    let id = item.itemID as usize;
    let Some(scale) = menu_scale(item.itemData, id) else {
        return false;
    };
    let (name, description, shortcut) = menu_text(id).unwrap();
    let s = |v: i32| (v * scale + 48) / 96;
    let dc = item.hDC;
    let r = item.rcItem;
    let selected = item.itemState & ODS_SELECTED != 0;
    let disabled = item.itemState & (ODS_DISABLED | ODS_GRAYED) != 0;
    let color = if disabled {
        theme::DISABLED
    } else if selected {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    let saved = SaveDC(dc);
    fill(dc, r, theme::SURFACE);
    if selected {
        fill(
            dc,
            RECT {
                left: r.left + s(3),
                top: r.top + s(2),
                right: r.right - s(3),
                bottom: r.bottom - s(2),
            },
            theme::SELECTION,
        );
    }
    SetBkMode(dc, TRANSPARENT as i32);
    if item.itemState & ODS_CHECKED != 0 {
        let pen = CreatePen(PS_SOLID, s(2).max(1), color);
        let old_pen = SelectObject(dc, pen);
        let y = (r.top + r.bottom) / 2;
        MoveToEx(dc, r.left + s(9), y, null_mut());
        LineTo(dc, r.left + s(13), y + s(4));
        LineTo(dc, r.left + s(20), y - s(4));
        SelectObject(dc, old_pen);
        DeleteObject(pen);
    }
    icon(
        dc,
        id,
        RECT {
            left: r.left + s(32),
            top: r.top + s(14),
            right: r.left + s(56),
            bottom: r.top + s(38),
        },
        color,
        scale,
    );
    let title_font = menu_font(data, s(15));
    let description_font = menu_font(data, s(12));
    SelectObject(
        dc,
        if title_font.is_null() {
            data.font
        } else {
            title_font
        },
    );
    SetTextColor(dc, color);
    let mut title_rect = RECT {
        left: r.left + s(70),
        top: r.top + s(5),
        right: r.right - s(40),
        bottom: r.top + s(28),
    };
    DrawTextW(
        dc,
        wide(name).as_ptr(),
        -1,
        &mut title_rect,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );
    let mut shortcut_rect = RECT {
        left: r.right - s(36),
        right: r.right - s(14),
        ..title_rect
    };
    SetTextColor(
        dc,
        if disabled {
            theme::DISABLED
        } else {
            theme::MUTED
        },
    );
    DrawTextW(
        dc,
        wide(shortcut).as_ptr(),
        -1,
        &mut shortcut_rect,
        DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
    );
    SelectObject(
        dc,
        if description_font.is_null() {
            data.font
        } else {
            description_font
        },
    );
    let mut description_rect = RECT {
        left: title_rect.left,
        top: r.top + s(27),
        right: r.right - s(14),
        bottom: r.bottom - s(5),
    };
    DrawTextW(
        dc,
        wide(description).as_ptr(),
        -1,
        &mut description_rect,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    RestoreDC(dc, saved);
    if !title_font.is_null() {
        DeleteObject(title_font);
    }
    if !description_font.is_null() {
        DeleteObject(description_font);
    }
    true
}
pub(super) unsafe fn click(owner: HWND, source: HWND, p: [i32; 2]) -> Result<()> {
    // A native popup dispatches owner-window messages. End the View borrow before
    // entering its message loop, then reacquire it only when a tool was chosen.
    let (hit, scale, button, choice, available) = {
        let ptr = view(owner);
        if ptr.is_null() {
            return Ok(());
        }
        let data = &*ptr;
        let layout = layout(source, data);
        let Some(hit) = layout.split_hit(p, &data.group_tools) else {
            return Ok(());
        };
        match hit {
            ButtonHit::Command(id) => {
                if !enabled(data, id) {
                    return Ok(());
                }
                (hit, layout.scale, RECT::default(), 0, vec![])
            }
            ButtonHit::Dropdown(group) => {
                let anchor = GROUP_ANCHORS[group];
                if !enabled(data, anchor) {
                    return Ok(());
                }
                let button = layout
                    .buttons
                    .iter()
                    .find(|&&(id, _)| id == anchor)
                    .unwrap()
                    .1;
                (
                    hit,
                    layout.scale,
                    button,
                    group_choice(&data.group_tools, group),
                    GROUPS[group].iter().map(|&id| enabled(data, id)).collect(),
                )
            }
        }
    };
    let ButtonHit::Dropdown(group) = hit else {
        let ButtonHit::Command(id) = hit else {
            unreachable!()
        };
        return command(owner, id);
    };
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return Err("无法打开标注工具菜单".into());
    }
    for (index, &id) in GROUPS[group].iter().enumerate() {
        let (name, _, _) = menu_text(id).unwrap();
        let mut text = wide(name);
        let item = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_DATA | MIIM_STATE | MIIM_STRING,
            fType: MFT_OWNERDRAW,
            fState: if available[index] {
                MFS_ENABLED
            } else {
                MFS_DISABLED
            } | if id == choice {
                MFS_CHECKED
            } else {
                MFS_UNCHECKED
            },
            wID: id as u32,
            dwItemData: MENU_DATA_TAG | ((scale as usize) << 8) | id,
            dwTypeData: text.as_mut_ptr(),
            cch: (text.len() - 1) as u32,
            ..zeroed()
        };
        InsertMenuItemW(menu, index as u32, 1, &item);
    }
    let mut origin = POINT {
        x: button.left,
        y: button.bottom,
    };
    ClientToScreen(source, &mut origin);
    let exclude = RECT {
        left: origin.x,
        top: origin.y - (button.bottom - button.top),
        right: origin.x + button.right - button.left,
        bottom: origin.y,
    };
    let params = TPMPARAMS {
        cbSize: size_of::<TPMPARAMS>() as u32,
        rcExclude: exclude,
    };
    // The floating toolbar does not activate its pin on ordinary clicks. Give
    // the popup's owner foreground input so menu keys and outside clicks work.
    SetForegroundWindow(owner);
    let id = TrackPopupMenuEx(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN | TPM_TOPALIGN,
        origin.x,
        origin.y,
        owner,
        &params,
    );
    DestroyMenu(menu);
    if IsWindow(owner) != 0 {
        PostMessageW(owner, WM_NULL, 0, 0);
    }
    if id != 0 && IsWindow(owner) != 0 && GROUPS[group].contains(&(id as usize)) {
        command(owner, id as usize)?;
    }
    Ok(())
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
        info.rcWork.bottom - info.rcWork.top,
        GetDpiForWindow(owner),
        None,
    );
    let width = layout.bounds.right;
    let height = layout.bounds.bottom;
    let gap = (8 * layout.scale + 48) / 96;
    let x = own.left.clamp(
        info.rcWork.left,
        (info.rcWork.right - width).max(info.rcWork.left),
    );
    let y = if own.bottom + gap + height <= info.rcWork.bottom {
        own.bottom + gap
    } else {
        (own.top - height - gap).max(info.rcWork.top)
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
        if msg == WM_LBUTTONDOWN {
            if let Err(e) = click(owner, hwnd, point(l)) {
                alert(owner, &e);
            }
            return 0;
        }
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
        for dpi in [96, 144, 192, 288] {
            for (width, height) in [(320, 600), (320, 1080), (800, 1080), (1920, 1080)] {
                let layout = Layout::new(
                    width,
                    height,
                    dpi,
                    Some(RECT {
                        left: 10,
                        top: 100,
                        right: 200,
                        bottom: 1070,
                    }),
                );
                assert!(layout.bounds.left >= 0 && layout.bounds.top >= 0);
                assert!(layout.bounds.right <= width);
                assert!(layout.bounds.bottom <= height);
                for &(id, r) in &layout.buttons {
                    assert!(r.right <= layout.bounds.right && r.bottom <= layout.bounds.bottom);
                    assert!(r.left >= layout.bounds.left && r.top >= layout.bounds.top);
                    assert!(r.bottom <= layout.status.top);
                    assert_eq!(
                        layout.hit([(r.left + r.right) / 2, (r.top + r.bottom) / 2]),
                        Some(id)
                    );
                }
            }
        }
    }

    #[test]
    fn toolbar_exposes_recognition_and_screen_actions_without_a_more_menu() {
        let layout = Layout::new(800, 600, 144, None);
        let ids: Vec<_> = layout.buttons.iter().map(|&(id, _)| id).collect();
        for id in [12, 13, 14, 15, 16, 19, 20, 24, 25, 38, 101] {
            assert!(ids.contains(&id), "missing action {id}");
        }
        assert!(!ids.contains(&8), "mosaic is a brush style");
        assert!(!ids.contains(&23), "all actions have their own icon");
        assert!(
            layout
                .buttons
                .windows(2)
                .any(|pair| pair[0].1.top != pair[1].1.top)
        );
    }

    #[test]
    fn grouped_buttons_use_separate_main_and_dropdown_targets_at_multiple_dpi() {
        let choices = [2, 18, 5];
        for dpi in [96, 144, 192, 288] {
            for (width, height) in [(320, 600), (800, 1080), (1920, 1080)] {
                let layout = Layout::new(width, height, dpi, None);
                for (group, &anchor) in GROUP_ANCHORS.iter().enumerate() {
                    let button = layout
                        .buttons
                        .iter()
                        .find(|&&(id, _)| id == anchor)
                        .unwrap()
                        .1;
                    let arrow = layout.arrow_rect(button);
                    let y = (button.top + button.bottom) / 2;
                    assert!(arrow.left > button.left && arrow.right == button.right);
                    assert_eq!(
                        layout.split_hit([(button.left + arrow.left) / 2, y], &choices),
                        Some(ButtonHit::Command(choices[group]))
                    );
                    assert_eq!(
                        layout.split_hit([arrow.left, y], &choices),
                        Some(ButtonHit::Dropdown(group))
                    );
                    assert_eq!(layout.hit([arrow.left, y]), Some(anchor));
                }
            }
        }
    }

    #[test]
    fn grouped_toolbar_remembers_each_choice_and_updates_its_icon() {
        let mut data = empty_view(RgbaImage::new(32, 32));
        assert_eq!(data.group_tools, [1, 3, 4]);
        for (id, anchor, tool) in [
            (2, 1, Tool::Ellipse),
            (17, 3, Tool::Line),
            (18, 3, Tool::Polyline),
            (5, 4, Tool::Highlight),
        ] {
            remember_tool(&mut data, id);
            data.tool = tool;
            assert_eq!(displayed_tool(&data, anchor), id);
            assert!(active_tool(&data, anchor));
        }
        assert_eq!(data.group_tools, [2, 18, 5]);
        remember_tool(&mut data, 6);
        assert_eq!(data.group_tools, [2, 18, 5]);
        remember_tool(&mut data, 38);
        assert_eq!(data.group_tools, [2, 18, 4]);
        data.tool = Tool::Text;
        assert!(!active_tool(&data, 1));
        assert!(!active_tool(&data, 3));
        assert!(!active_tool(&data, 4));
        assert!(active_tool(&data, 6));
    }

    #[test]
    fn grouped_choices_fall_back_to_valid_defaults_and_remove_duplicate_cells() {
        let layout = Layout::new(1920, 1080, 96, None);
        let ids: Vec<_> = layout
            .buttons
            .iter()
            .take(ITEMS.len())
            .map(|&(id, _)| id)
            .collect();
        assert_eq!(ids.len(), 22);
        for id in [2, 17, 18, 5] {
            assert!(!ids.contains(&id));
        }
        for (group, &anchor) in GROUP_ANCHORS.iter().enumerate() {
            assert_eq!(group_choice(&[100, 100, 100], group), anchor);
            assert!(ids.contains(&anchor));
        }
    }

    #[test]
    fn dropdown_menu_measurement_handles_only_its_own_rows_and_scales_with_dpi() {
        for scale in [96, 144, 192, 288] {
            for &id in GROUPS.iter().flat_map(|group| group.iter()) {
                let mut row = MEASUREITEMSTRUCT {
                    CtlType: ODT_MENU,
                    itemID: id as u32,
                    itemData: MENU_DATA_TAG | ((scale as usize) << 8) | id,
                    ..unsafe { zeroed() }
                };
                assert!(measure_menu(&mut row));
                assert_eq!(row.itemHeight, ((52 * scale + 48) / 96) as u32);
                assert_eq!(row.itemWidth, ((260 * scale + 48) / 96) as u32);
            }
        }
        let mut unrelated: MEASUREITEMSTRUCT = unsafe { zeroed() };
        unrelated.CtlType = ODT_MENU;
        unrelated.itemID = 1;
        assert!(!measure_menu(&mut unrelated));
        unrelated.itemData = MENU_DATA_TAG | (96 << 8) | 1;
        unrelated.CtlType = ODT_LISTBOX;
        assert!(!measure_menu(&mut unrelated));
    }

    #[test]
    fn toolbar_pin_window_keeps_the_same_size_when_using_its_client_bounds() {
        for (width, height, dpi) in [(320, 600, 192), (800, 600, 288), (1920, 1080, 192)] {
            let screen = Layout::new(width, height, dpi, None);
            let window = Layout::new(screen.bounds.right, screen.bounds.bottom, dpi, None);
            assert_eq!(screen.bounds.right, window.bounds.right);
            assert_eq!(screen.bounds.bottom, window.bounds.bottom);
            assert_eq!(screen.scale, window.scale);
        }
    }

    #[test]
    fn original_file_action_is_visible_and_only_enabled_for_file_backed_images() {
        let layout = Layout::new(320, 600, 288, None);
        let mut data = empty_view(RgbaImage::new(32, 32));
        let button = layout.buttons.iter().find(|&&(id, _)| id == 101).unwrap().1;
        assert_eq!(
            layout.hit([
                (button.left + button.right) / 2,
                (button.top + button.bottom) / 2
            ]),
            Some(101)
        );
        assert!(!enabled(&data, 101));
        data.file = Some(PathBuf::from("example.png"));
        assert!(enabled(&data, 101));
    }
}
