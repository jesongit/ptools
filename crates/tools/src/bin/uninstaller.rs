#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use ptools_tools::{interactive::Invocation, uninstaller};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--cleanup") {
        return uninstaller::cleanup_plan(std::path::Path::new(args.get(1).ok_or("缺少清理计划")?));
    }
    if args.iter().any(|s| s == "--list") {
        println!(
            "{}",
            serde_json::to_string_pretty(&uninstaller::enumerate()).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--interactive") {
        return uninstaller::run(None, false);
    }
    let root = if let Some(i) = args.iter().position(|s| s == "--data-dir") {
        std::path::absolute(args.get(i + 1).ok_or("缺少数据目录")?).map_err(|e| e.to_string())?
    } else {
        std::path::PathBuf::from(std::env::var_os("APPDATA").ok_or("找不到用户目录")?)
            .join("ptools/plugin-data/uninstaller")
    };
    uninstaller::run(
        Some(Invocation {
            protocol_version: 2,
            operation: "invoke".into(),
            action: "open".into(),
            data_dir: root,
            region: None,
        }),
        args.iter().any(|s| s == "--smoke-ui"),
    )
}
