use serde::Deserialize;
use std::{
    io::{BufRead, Read},
    path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const WM_INVOKE: u32 = WM_APP + 20;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub protocol_version: u32,
    pub operation: String,
    pub action: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub region: Option<[i32; 4]>,
}

pub fn read_invocations(hwnd: HWND) {
    let window = hwnd as usize;
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut bytes = Vec::new();
            let read = input.by_ref().take(65537).read_until(b'\n', &mut bytes);
            match read {
                Ok(0) | Err(_) => {
                    unsafe {
                        PostMessageW(window as HWND, WM_INVOKE, 0, 0);
                    }
                    break;
                }
                Ok(_) if bytes.len() <= 65536 && bytes.last() == Some(&b'\n') => {
                    if let Ok(request) = serde_json::from_slice::<Invocation>(&bytes)
                        && request.protocol_version == 2
                        && request.operation == "invoke"
                        && request.data_dir.is_absolute()
                    {
                        let ptr = Box::into_raw(Box::new(request));
                        if unsafe { PostMessageW(window as HWND, WM_INVOKE, 0, ptr as isize) } == 0
                        {
                            unsafe {
                                drop(Box::from_raw(ptr));
                            }
                            break;
                        }
                    }
                }
                _ => {
                    unsafe {
                        PostMessageW(window as HWND, WM_INVOKE, 0, 0);
                    }
                    break;
                }
            }
        }
    });
}
