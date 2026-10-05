#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bundled;
mod calculator;
mod command;
mod gesture;
mod input;
mod interactive;
mod ui;
use ptools_core::*;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("ptools: {error}");
        if std::env::args().len() == 1 {
            ui::error_box(&error);
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|x| x == "--help" || x == "-h") {
        println!(
            "ptools {} — 原生应用启动器\n\n启动后直接驻留托盘；Alt+Space 或点击托盘显示搜索框；Esc 隐藏。\n\n--portable             数据保存在程序旁 data/\n--data-dir PATH        指定数据目录\n--hidden               启动后驻留托盘（默认）\n--show                 启动时显示搜索框\n--refresh-index        刷新应用索引并退出\n--search QUERY         搜索缓存，输出 JSON\n--plugins              列出已安装插件\n--install PATH         安装目录、plugin.json 或 .ptplugin/.zip 包\n--uninstall ID         卸载插件和对应索引\n--doctor               输出版本、数据目录与索引状态\n--smoke-ui             显示原生界面后自动退出（验证用）",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let value = |flag: &str| -> Result<Option<String>> {
        if let Some(i) = args.iter().position(|x| x == flag) {
            Ok(Some(
                args.get(i + 1)
                    .filter(|s| !s.starts_with("--"))
                    .ok_or_else(|| format!("{flag} 缺少参数"))?
                    .clone(),
            ))
        } else {
            Ok(None)
        }
    };
    let exe_dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("程序目录不存在")?
        .to_owned();
    let paths = Paths::resolve(
        &exe_dir,
        args.iter().any(|s| s == "--portable"),
        value("--data-dir")?.map(PathBuf::from),
    )?;
    let mut settings: Settings = read_json(&paths.settings())?;
    if !ui::is_running(&paths) {
        bundled::sync(&exe_dir, &paths)?;
    }
    let mut cache: Cache = read_json(&paths.cache())?;
    if args.iter().any(|x| x == "--doctor") {
        let (plugins, errors) = discover_plugins(&paths.plugins);
        println!(
            "{}",
            serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"data_dir":paths.root,"portable":paths.portable,"hotkey":settings.hotkey,"plugins":plugins.iter().map(|p| &p.manifest).collect::<Vec<_>>(),"entry_count":cache.plugins.values().map(Vec::len).sum::<usize>(),"updated_at":cache.updated_at,"errors":errors})
        );
        return Ok(());
    }
    if args.iter().any(|x| x == "--plugins") {
        let (plugins, errors) = discover_plugins(&paths.plugins);
        println!(
            "{}",
            serde_json::json!({"plugins":plugins.iter().map(|p| &p.manifest).collect::<Vec<_>>(),"errors":errors})
        );
        return Ok(());
    }
    if let Some(query) = value("--search")? {
        let index = build_index(&cache, &settings);
        let hits: Vec<_> = search(&index, &query, &settings, 50)
            .into_iter()
            .map(|i| &index[i].entry)
            .collect();
        println!(
            "{}",
            serde_json::to_string(&hits).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let install = value("--install")?;
    let uninstall = value("--uninstall")?;
    let refresh_requested = args.iter().any(|x| x == "--refresh-index");
    if install.is_some() || uninstall.is_some() || refresh_requested {
        if ui::is_running(&paths) {
            return Err(
                "ptools 正在运行，请在搜索框中输入“插件”或“刷新”操作，或退出后使用命令行".into(),
            );
        }
        if let Some(source) = install {
            let manifest = install_plugin(&PathBuf::from(source), &paths.plugins)?;
            settings.disabled_plugins.remove(&manifest.id);
            write_json(&paths.settings(), &settings)?;
            println!("已安装 {} ({})", manifest.name, manifest.id);
        }
        if let Some(id) = uninstall {
            uninstall_plugin(&paths.plugins, &id)?;
            cache.plugins.remove(&id);
            settings.disabled_plugins.remove(&id);
            settings
                .usage
                .retain(|key, _| !key.starts_with(&format!("{id}:")));
            write_json(&paths.settings(), &settings)?;
        }
        let (next, errors) = refresh(&paths, &settings, &cache);
        write_json(&paths.cache(), &next)?;
        write_json(&paths.root.join("last-refresh.json"), &errors)?;
        println!(
            "{}",
            serde_json::json!({"entries":next.plugins.values().map(Vec::len).sum::<usize>(),"errors":errors})
        );
        return if errors.is_empty() {
            Ok(())
        } else {
            Err("索引刷新有警告，详见输出或 last-refresh.json".into())
        };
    }
    ui::run(
        paths,
        settings,
        cache,
        !args.iter().any(|s| s == "--show" || s == "--smoke-ui"),
        args.iter().any(|s| s == "--smoke-ui"),
    )
}
