#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ptools_core::{Entry, IndexRequest, IndexResponse, Target};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_CommonPrograms, FOLDERID_Desktop,
            FOLDERID_Programs, FOLDERID_PublicDesktop, IEnumShellItems, IShellItem, IShellLinkW,
            SHCreateItemInKnownFolder, SHGetKnownFolderPath, SIGDN_DESKTOPABSOLUTEPARSING,
            SIGDN_NORMALDISPLAY, SLGP_RAWPATH, ShellLink,
        },
    },
    core::{Interface, PCWSTR},
};

fn application_link(path: &Path) -> bool {
    let check = || -> windows::core::Result<bool> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let file: IPersistFile = link.cast()?;
            let name: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            file.Load(PCWSTR(name.as_ptr()), STGM_READ)?;
            let mut target = vec![0u16; 32768];
            link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)?;
            let target = String::from_utf16_lossy(
                &target[..target.iter().position(|c| *c == 0).unwrap_or(0)],
            );
            // Empty paths can be advertised / packaged shell applications.
            if target.is_empty() {
                return Ok(true);
            }
            let extension = Path::new(&target)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            Ok(["exe", "com", "bat", "cmd", "msc", "cpl", "appref-ms"]
                .contains(&extension.as_str()))
        }
    };
    check().unwrap_or(false)
}

fn visit(path: &Path, entries: &mut BTreeMap<String, Entry>, depth: usize) {
    if depth > 12 || entries.len() >= 20000 {
        return;
    }
    let Ok(dir) = fs::read_dir(path) else {
        return;
    };
    for item in dir.flatten() {
        let path = item.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            continue;
        }
        if meta.is_dir() {
            visit(&path, entries, depth + 1);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !["lnk", "url", "appref-ms"].contains(&ext.as_str()) {
            continue;
        }
        if ext == "lnk" && !application_link(&path) {
            continue;
        }
        let Some(title) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if title.trim().is_empty() {
            continue;
        }
        let key = title.to_lowercase();
        let target = path.to_string_lossy().replace('/', "\\");
        entries.entry(key).or_insert_with(|| Entry {
            id: format!("path:{}", target.to_lowercase()),
            title: title.to_string(),
            subtitle: target.clone(),
            keywords: vec![],
            target: Target::Path { path: target },
        });
    }
}

unsafe fn display_name(
    item: &IShellItem,
    kind: windows::Win32::UI::Shell::SIGDN,
) -> windows::core::Result<String> {
    unsafe {
        let ptr = item.GetDisplayName(kind)?;
        let result = ptr.to_string();
        CoTaskMemFree(Some(ptr.as_ptr().cast()));
        Ok(result?)
    }
}

fn shell_apps(entries: &mut BTreeMap<String, Entry>) -> windows::core::Result<()> {
    unsafe {
        let folder: IShellItem =
            SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, Default::default(), PCWSTR::null())?;
        let enumerator: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        loop {
            let mut items: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0;
            let result = enumerator.Next(&mut items, Some(&mut fetched));
            if result.is_err() || fetched == 0 {
                break;
            }
            if let Some(item) = items[0].take()
                && let (Ok(title), Ok(app_id)) = (
                    display_name(&item, SIGDN_NORMALDISPLAY),
                    display_name(&item, SIGDN_DESKTOPABSOLUTEPARSING),
                )
            {
                if title.is_empty() || app_id.is_empty() {
                    continue;
                }
                let app_id = app_id
                    .strip_prefix("shell:AppsFolder\\")
                    .unwrap_or(&app_id)
                    .to_string();
                // Shell may expose filesystem paths for unpackaged applications; retain those as paths.
                let target = if Path::new(&app_id).is_absolute() {
                    Target::Path {
                        path: app_id.clone(),
                    }
                } else if app_id.contains(['/', '\\', '"', '\r', '\n']) {
                    continue;
                } else {
                    Target::App {
                        app_id: app_id.clone(),
                    }
                };
                entries.entry(title.to_lowercase()).or_insert(Entry {
                    id: format!("app:{app_id}"),
                    title,
                    subtitle: "系统应用".into(),
                    keywords: vec![],
                    target,
                });
            }
            if entries.len() >= 20000 {
                break;
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .take(65536)
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    let request: IndexRequest = serde_json::from_str(&input).map_err(|e| e.to_string())?;
    if request.protocol_version != 1 || request.operation != "index" {
        return Err("Unsupported request".into());
    }
    let mut entries = BTreeMap::new();
    // Built-in names and aliases take precedence over shortcuts with the same title.
    for command in ptools_core::SYSTEM_COMMANDS {
        if command.available() {
            entries.insert(command.title.to_lowercase(), command.entry());
        }
    }
    let mut warnings = Vec::new();
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft/Windows/Start Menu/Programs"));
    }
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        roots.push(PathBuf::from(programdata).join("Microsoft/Windows/Start Menu/Programs"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        roots.push(PathBuf::from(profile).join("Desktop"));
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push(PathBuf::from(public).join("Desktop"));
    }
    for folder in [
        &FOLDERID_Programs,
        &FOLDERID_CommonPrograms,
        &FOLDERID_Desktop,
        &FOLDERID_PublicDesktop,
    ] {
        unsafe {
            if let Ok(pointer) = SHGetKnownFolderPath(folder, Default::default(), None) {
                if let Ok(path) = pointer.to_string() {
                    roots.push(PathBuf::from(path));
                }
                CoTaskMemFree(Some(pointer.as_ptr().cast()));
            }
        }
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        visit(&root, &mut entries, 0);
    }
    unsafe {
        if initialized {
            if let Err(e) = shell_apps(&mut entries) {
                warnings.push(format!("系统应用枚举失败，快捷方式仍可使用：{e}"));
            }
            CoUninitialize();
        } else {
            warnings.push("无法初始化系统应用枚举".into());
        }
    }
    let response = IndexResponse {
        protocol_version: 1,
        entries: entries.into_values().collect(),
        warnings,
    };
    let bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguish_application_and_folder_shortcuts() {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
            let root = std::env::temp_dir().join(format!("ptools-links-{}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            for (name, target) in [
                ("application", std::env::current_exe().unwrap()),
                ("folder", root.clone()),
            ] {
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                let target: Vec<u16> = target
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                link.SetPath(PCWSTR(target.as_ptr())).unwrap();
                let file: IPersistFile = link.cast().unwrap();
                let destination = root.join(format!("{name}.lnk"));
                let path: Vec<u16> = destination
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                file.Save(PCWSTR(path.as_ptr()), true).unwrap();
            }
            assert!(application_link(&root.join("application.lnk")));
            assert!(!application_link(&root.join("folder.lnk")));
            let mut entries = BTreeMap::new();
            visit(&root, &mut entries, 0);
            assert_eq!(entries.len(), 1);
            assert!(entries.contains_key("application"));
            fs::remove_dir_all(root).unwrap();
            CoUninitialize();
        }
    }
}
