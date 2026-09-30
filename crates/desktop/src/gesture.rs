//! Win + left drag. Hooks run on a small message thread, never on the busy UI thread.
#![allow(unsafe_op_in_unsafe_fn)]

use ptools_core::Result;
use std::{
    cell::Cell,
    collections::VecDeque,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::DwmFlush, Gdi::*},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const WM_QUICK_PIN: u32 = WM_APP + 6;
const UPDATE: u32 = WM_APP + 40;
const FINISH: u32 = WM_APP + 41;
const CLEAN_KEYS: u32 = WM_APP + 42;

pub struct Gesture {
    thread: u32,
    worker: Option<JoinHandle<()>>,
    regions: Receiver<[i32; 4]>,
}

impl Gesture {
    pub fn start(hwnd: HWND) -> Result<Self> {
        let window = hwnd as usize;
        let (ready_tx, ready_rx) = mpsc::channel();
        let (regions_tx, regions_rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("capture-gesture".into())
            .stack_size(128 * 1024)
            .spawn(move || unsafe { run(window as HWND, ready_tx, regions_tx) })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv().map_err(|e| e.to_string())? {
            Ok(thread) => Ok(Self {
                thread,
                worker: Some(worker),
                regions: regions_rx,
            }),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
    }

    pub fn region(&self) -> Option<[i32; 4]> {
        self.regions.try_recv().ok()
    }
}

impl Drop for Gesture {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread, WM_QUIT, 0, 0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Drag {
    start: POINT,
    end: POINT,
    cancelled: bool,
}

impl Drag {
    fn region(&self) -> Option<[i32; 4]> {
        let left = self.start.x.min(self.end.x);
        let top = self.start.y.min(self.end.y);
        let width = self.start.x.max(self.end.x).checked_sub(left)?;
        let height = self.start.y.max(self.end.y).checked_sub(top)?;
        (!self.cancelled && width >= 4 && height >= 4).then_some([left, top, width, height])
    }
}

struct Listener {
    thread: u32,
    mouse: HHOOK,
    keyboard: HHOOK,
    outline: HWND,
    drag: Option<Drag>,
    pending: VecDeque<[i32; 4]>,
    update_queued: bool,
    mask_win: bool,
    escape_down: bool,
}

thread_local! { static LISTENER: Cell<*mut Listener> = const { Cell::new(null_mut()) }; }

unsafe fn down(key: u16) -> bool {
    GetAsyncKeyState(key as i32) < 0
}
unsafe fn win_down() -> bool {
    down(VK_LWIN) || down(VK_RWIN)
}

unsafe fn queue_update(state: &mut Listener) {
    if !state.update_queued {
        state.update_queued = true;
        PostThreadMessageW(state.thread, UPDATE, 0, 0);
    }
}

unsafe fn mask_start_menu() -> bool {
    // vkE8 is unassigned. Like menu masking in AutoHotkey, this marks Win as used
    // without faking a Ctrl/Alt press or blocking the real Win key-up.
    let keys = [0, KEYEVENTF_KEYUP].map(|flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0xe8,
                dwFlags: flags,
                ..zeroed()
            },
        },
    });
    SendInput(2, keys.as_ptr(), size_of::<INPUT>() as i32) == 2
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let ptr = LISTENER.with(Cell::get);
    if ptr.is_null() {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let state = &mut *ptr;
    let event = &*(l as *const MSLLHOOKSTRUCT);
    if w as u32 == WM_LBUTTONDOWN
        && state.drag.is_none()
        && win_down()
        && !down(VK_CONTROL)
        && !down(VK_MENU)
        && !down(VK_SHIFT)
    {
        if state.keyboard.is_null() {
            state.keyboard = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook),
                GetModuleHandleW(null()),
                0,
            );
        }
        if !state.keyboard.is_null() && mask_start_menu() {
            state.mask_win = true;
            state.drag = Some(Drag {
                start: event.pt,
                end: event.pt,
                cancelled: false,
            });
            queue_update(state);
            return 1;
        }
        PostThreadMessageW(state.thread, CLEAN_KEYS, 0, 0);
    }
    if let Some(drag) = &mut state.drag {
        match w as u32 {
            WM_MOUSEMOVE => {
                drag.end = event.pt;
                if !win_down() {
                    drag.cancelled = true;
                }
                queue_update(state);
                // Keep cursor movement live. Only the down/up pair is consumed,
                // so the underlying application never begins a drag.
            }
            WM_LBUTTONUP => {
                drag.end = event.pt;
                if !win_down() {
                    drag.cancelled = true;
                }
                if let Some(region) = drag.region() {
                    state.pending.push_back(region);
                }
                state.drag = None;
                PostThreadMessageW(state.thread, FINISH, 0, 0);
                PostThreadMessageW(state.thread, CLEAN_KEYS, 0, 0);
                return 1;
            }
            _ => {}
        }
    }
    CallNextHookEx(null_mut(), code, w, l)
}

unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let key = (*(l as *const KBDLLHOOKSTRUCT)).vkCode as u16;
    if ![VK_LWIN, VK_RWIN, VK_ESCAPE].contains(&key) {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let ptr = LISTENER.with(Cell::get);
    if ptr.is_null() {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    let state = &mut *ptr;
    let released = matches!(w as u32, WM_KEYUP | WM_SYSKEYUP);
    if key == VK_LWIN || key == VK_RWIN {
        if released {
            let other = if key == VK_LWIN { VK_RWIN } else { VK_LWIN };
            if !down(other) {
                state.mask_win = false;
                if let Some(drag) = &mut state.drag {
                    drag.cancelled = true;
                    queue_update(state);
                }
                PostThreadMessageW(state.thread, CLEAN_KEYS, 0, 0);
            }
        } else if state.mask_win {
            // Prevent Win auto-repeat from making the Start menu eligible again.
            return 1;
        }
    } else if key == VK_ESCAPE && (state.drag.is_some() || state.escape_down) {
        state.escape_down = !released;
        if let Some(drag) = &mut state.drag {
            drag.cancelled = true;
            queue_update(state);
        }
        PostThreadMessageW(state.thread, CLEAN_KEYS, 0, 0);
        return 1;
    }
    CallNextHookEx(null_mut(), code, w, l)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

unsafe fn close_outline(state: &mut Listener) {
    if !state.outline.is_null() {
        DestroyWindow(state.outline);
        state.outline = null_mut();
    }
}

unsafe fn update_outline(state: &mut Listener) {
    state.update_queued = false;
    let Some(region) = state.drag.as_ref().and_then(Drag::region) else {
        close_outline(state);
        return;
    };
    let [x, y, width, height] = region;
    if state.outline.is_null() {
        let class = wide("ptools.capture.outline");
        let module = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(outline_proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        state.outline = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class.as_ptr(),
            wide("ptools 快速截图框选").as_ptr(),
            WS_POPUP | WS_DISABLED,
            x,
            y,
            width,
            height,
            null_mut(),
            null_mut(),
            module,
            null(),
        );
        if state.outline.is_null() {
            return;
        }
        SetLayeredWindowAttributes(state.outline, 0, 255, LWA_COLORKEY);
    }
    SetWindowPos(
        state.outline,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    InvalidateRect(state.outline, null(), 0);
}

unsafe extern "system" fn outline_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut paint: PAINTSTRUCT = zeroed();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r);
        FillRect(dc, &r, GetStockObject(BLACK_BRUSH) as HBRUSH);
        let brush = CreateSolidBrush(0x00f09030);
        FrameRect(dc, &r, brush);
        InflateRect(&mut r, -1, -1);
        FrameRect(dc, &r, brush);
        DeleteObject(brush);
        EndPaint(hwnd, &paint);
        return 0;
    }
    if msg == WM_ERASEBKGND {
        return 1;
    }
    DefWindowProcW(hwnd, msg, w, l)
}

unsafe fn run(hwnd: HWND, ready: Sender<Result<u32>>, regions: Sender<[i32; 4]>) {
    let thread = GetCurrentThreadId();
    let mut message: MSG = zeroed();
    PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
    let mut state = Box::new(Listener {
        thread,
        mouse: null_mut(),
        keyboard: null_mut(),
        outline: null_mut(),
        drag: None,
        pending: VecDeque::new(),
        update_queued: false,
        mask_win: false,
        escape_down: false,
    });
    LISTENER.with(|slot| slot.set(&mut *state));
    state.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), GetModuleHandleW(null()), 0);
    if state.mouse.is_null() {
        let _ = ready.send(Err(format!("无法启用 Win 拖动截图：{}", GetLastError())));
        LISTENER.with(|slot| slot.set(null_mut()));
        return;
    }
    let _ = ready.send(Ok(thread));
    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
        match message.message {
            UPDATE => update_outline(&mut state),
            FINISH => {
                if state.drag.is_none() {
                    close_outline(&mut state);
                    // Remove the border from the composed desktop before the plugin captures it.
                    DwmFlush();
                    for region in state.pending.drain(..) {
                        if regions.send(region).is_ok() {
                            PostMessageW(hwnd, WM_QUICK_PIN, 0, 0);
                        }
                    }
                }
            }
            CLEAN_KEYS => {
                if state.drag.is_none()
                    && !state.mask_win
                    && !state.escape_down
                    && !state.keyboard.is_null()
                {
                    UnhookWindowsHookEx(state.keyboard);
                    state.keyboard = null_mut();
                }
            }
            _ => {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    close_outline(&mut state);
    if !state.keyboard.is_null() {
        UnhookWindowsHookEx(state.keyboard);
    }
    UnhookWindowsHookEx(state.mouse);
    LISTENER.with(|slot| slot.set(null_mut()));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangles_support_all_directions_and_negative_monitors() {
        let start = POINT { x: -500, y: -300 };
        for end in [
            POINT { x: -700, y: -400 },
            POINT { x: -300, y: -200 },
            POINT { x: -700, y: -200 },
            POINT { x: -300, y: -400 },
        ] {
            let r = Drag {
                start,
                end,
                cancelled: false,
            }
            .region()
            .unwrap();
            assert_eq!(r[2..], [200, 100]);
            assert_eq!(r[0], start.x.min(end.x));
            assert_eq!(r[1], start.y.min(end.y));
        }
        for end in [start, POINT { x: -497, y: 500 }] {
            assert!(
                Drag {
                    start,
                    end,
                    cancelled: false
                }
                .region()
                .is_none()
            );
        }
        assert!(
            Drag {
                start,
                end: POINT { x: 0, y: 0 },
                cancelled: true
            }
            .region()
            .is_none()
        );
        assert!(
            Drag {
                start: POINT { x: i32::MIN, y: 0 },
                end: POINT {
                    x: i32::MAX,
                    y: 100
                },
                cancelled: false
            }
            .region()
            .is_none()
        );
    }
}
