#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ptools_tools::{capture, interactive::Invocation, native, ocr};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 0);
    }
    if args.first().map(String::as_str) == Some("--ocr") {
        let path = args.get(1).ok_or("缺少图片路径")?;
        let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        println!(
            "{}",
            serde_json::to_string(&ocr::recognize(&image)?).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--probe-ocr") {
        let image = unsafe { native::text_image("轻量离线识别测试 Hello ptools 12345") }?;
        let recognition = ocr::recognize(&image)?;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentPackageFullName(length: *mut u32, name: *mut u16) -> i32;
        }
        let mut length = 0;
        let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
        println!(
            "{}",
            serde_json::json!({"recognition":recognition,"has_package_identity":status!=15700,"package_query_status":status,"pid":std::process::id()})
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--interactive") {
        return capture::run(None, false);
    }
    let root = if let Some(i) = args.iter().position(|s| s == "--data-dir") {
        std::path::absolute(args.get(i + 1).ok_or("缺少数据目录")?).map_err(|e| e.to_string())?
    } else {
        std::path::PathBuf::from(std::env::var_os("APPDATA").ok_or("找不到用户目录")?)
            .join("ptools/plugin-data/capture")
    };
    let action = args
        .iter()
        .position(|s| s == "--action")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or("capture".into());
    capture::run(
        Some(Invocation {
            protocol_version: 2,
            operation: "invoke".into(),
            action,
            data_dir: root,
            region: None,
        }),
        args.iter().any(|s| s == "--smoke-ui"),
    )
}
