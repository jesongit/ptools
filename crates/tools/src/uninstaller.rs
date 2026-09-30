use crate::{
    interactive::{Invocation, WM_INVOKE, read_invocations},
    native::*,
};
use ptools_core::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, Registry::*, Threading::*},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const WM_DONE: u32 = WM_APP + 30;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Software {
    pub hive: String,
    pub key: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub size_kb: u32,
    pub date: String,
    pub location: String,
    pub command: String,
    pub msi: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ResidualTarget {
    Directory(PathBuf),
    File(PathBuf),
    Registry { hive: String, key: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Residual {
    pub software: Software,
    pub target: ResidualTarget,
    pub evidence: String,
    pub uncertain: bool,
}
impl Residual {
    fn label(&self) -> String {
        match &self.target {
            ResidualTarget::Directory(p) | ResidualTarget::File(p) => p.display().to_string(),
            ResidualTarget::Registry { hive, key } => format!("{hive}\\{key}"),
        }
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn hive(name: &str) -> Result<(HKEY, u32)> {
    match name {
        "HKCU64" => Ok((HKEY_CURRENT_USER, KEY_WOW64_64KEY)),
        "HKCU32" => Ok((HKEY_CURRENT_USER, KEY_WOW64_32KEY)),
        "HKLM64" => Ok((HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY)),
        "HKLM32" => Ok((HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY)),
        _ => Err("注册表来源无效".into()),
    }
}
unsafe fn open(hive_name: &str, path: &str, access: u32) -> Result<Key> {
    let (root, view) = hive(hive_name)?;
    let mut key = null_mut();
    let code = RegOpenKeyExW(root, wide(path).as_ptr(), 0, access | view, &mut key);
    if code == 0 {
        Ok(Key(key))
    } else {
        Err(format!("读取注册表失败：{code}"))
    }
}
unsafe fn string(key: HKEY, name: &str) -> String {
    let mut len = 0;
    let mut kind = 0;
    if RegQueryValueExW(
        key,
        wide(name).as_ptr(),
        null(),
        &mut kind,
        null_mut(),
        &mut len,
    ) != 0
        || !matches!(kind, REG_SZ | REG_EXPAND_SZ)
        || len > 32768
    {
        return String::new();
    }
    let mut bytes = vec![0u16; len as usize / 2 + 1];
    if RegQueryValueExW(
        key,
        wide(name).as_ptr(),
        null(),
        &mut kind,
        bytes.as_mut_ptr().cast(),
        &mut len,
    ) != 0
    {
        return String::new();
    }
    let value = String::from_utf16_lossy(
        &bytes[..bytes.iter().position(|c| *c == 0).unwrap_or(bytes.len())],
    );
    if kind == REG_EXPAND_SZ {
        expand(&value)
    } else {
        value
    }
}
unsafe fn number(key: HKEY, name: &str) -> u32 {
    let mut value = 0u32;
    let mut len = 4;
    let mut kind = 0;
    if RegQueryValueExW(
        key,
        wide(name).as_ptr(),
        null(),
        &mut kind,
        (&mut value as *mut u32).cast(),
        &mut len,
    ) == 0
        && kind == REG_DWORD
    {
        value
    } else {
        0
    }
}
fn expand(value: &str) -> String {
    let mut out = value.to_owned();
    for (name, value) in std::env::vars() {
        let pattern = format!("%{name}%");
        out = out.replace(&pattern, &value);
    }
    out
}
pub fn enumerate() -> Vec<Software> {
    let mut output = vec![];
    unsafe {
        for name in ["HKCU64", "HKCU32", "HKLM64", "HKLM32"] {
            let Ok(root) = open(name, UNINSTALL, KEY_READ) else {
                continue;
            };
            for i in 0..20000 {
                let mut key_name = vec![0u16; 256];
                let mut length = 255;
                let code = RegEnumKeyExW(
                    root.0,
                    i,
                    key_name.as_mut_ptr(),
                    &mut length,
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                if code == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if code != 0 {
                    continue;
                }
                let key_name = String::from_utf16_lossy(&key_name[..length as usize]);
                let key_path = format!("{UNINSTALL}\\{key_name}");
                let Ok(key) = open(name, &key_path, KEY_READ) else {
                    continue;
                };
                let display = string(key.0, "DisplayName");
                let command = string(key.0, "UninstallString");
                if display.is_empty()
                    || command.is_empty()
                    || number(key.0, "SystemComponent") == 1
                    || !string(key.0, "ParentKeyName").is_empty()
                {
                    continue;
                }
                output.push(Software {
                    hive: name.into(),
                    key: key_path,
                    name: display,
                    version: string(key.0, "DisplayVersion"),
                    publisher: string(key.0, "Publisher"),
                    size_kb: number(key.0, "EstimatedSize"),
                    date: string(key.0, "InstallDate"),
                    location: string(key.0, "InstallLocation")
                        .trim_matches('"')
                        .trim_end_matches(['\\', '/'])
                        .to_owned(),
                    command,
                    msi: number(key.0, "WindowsInstaller") == 1,
                });
            }
        }
    }
    output.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.hive.cmp(&b.hive))
    });
    output.dedup_by(|a, b| a.name == b.name && a.command == b.command);
    output
}
fn component(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2
        || value.len() > 200
        || value.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        || value == "."
        || value == ".."
    {
        None
    } else {
        Some(value.to_owned())
    }
}
fn protected(path: &Path) -> bool {
    if !path.is_absolute()
        || path.parent().is_none()
        || path.components().count() < 3
        || path.to_string_lossy().starts_with("\\\\?\\")
        || path.to_string_lossy().starts_with("\\\\.\\")
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return true;
    }
    let value = path
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    for name in [
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
        "SystemRoot",
    ] {
        if let Some(root) = std::env::var_os(name) {
            let root = PathBuf::from(root);
            let text = root
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase();
            if value == text
                || (matches!(name, "WINDIR" | "SystemRoot")
                    && value.starts_with(&format!("{text}\\")))
            {
                return true;
            }
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        for folder in [
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Videos",
            "Music",
            "AppData",
        ] {
            if same_path(path, &PathBuf::from(&profile).join(folder)) {
                return true;
            }
        }
    }
    if value.ends_with("\\start menu")
        || value.ends_with("\\start menu\\programs")
        || value.ends_with("\\desktop")
    {
        return true;
    }
    value.ends_with("\\common files")
        || value.ends_with("\\microsoft")
        || value.ends_with("\\windowsapps")
        || value.ends_with("\\windows")
}
fn no_links(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if let Ok(meta) = fs::symlink_metadata(&current)
            && (meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0)
        {
            return false;
        }
    }
    true
}
fn allowed_directory(software: &Software, path: &Path) -> bool {
    if protected(path) || !no_links(path) {
        return false;
    }
    if !software.location.is_empty() {
        let location = PathBuf::from(&software.location);
        if location.is_absolute() && same_path(&location, path) {
            return true;
        }
    }
    let Some(name) = component(&software.name) else {
        return false;
    };
    ["APPDATA", "LOCALAPPDATA", "ProgramData"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .any(|root| {
            same_path(&root.join(&name), path)
                || component(&software.publisher)
                    .is_some_and(|publisher| same_path(&root.join(publisher).join(&name), path))
        })
}
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
}
fn path_contains(parent: &Path, child: &Path) -> bool {
    let parent = parent
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    let child = child
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    child == parent || child.starts_with(&format!("{parent}\\"))
}
fn shortcut_roots() -> Vec<PathBuf> {
    let mut roots = vec![];
    for name in ["USERPROFILE", "PUBLIC"] {
        if let Some(root) = std::env::var_os(name) {
            roots.push(PathBuf::from(root).join("Desktop"));
        }
    }
    for name in ["APPDATA", "ProgramData"] {
        if let Some(root) = std::env::var_os(name) {
            roots.push(PathBuf::from(root).join("Microsoft/Windows/Start Menu/Programs"));
        }
    }
    roots
}
fn allowed_file(software: &Software, path: &Path) -> bool {
    if !path.is_absolute() || !no_links(path) {
        return false;
    }
    let Some(name) = component(&software.name) else {
        return false;
    };
    shortcut_roots().iter().any(|root| {
        same_path(path, &root.join(format!("{name}.lnk")))
            || same_path(path, &root.join(&name).join(format!("{name}.lnk")))
    })
}
fn registry_allowed(software: &Software, hive_name: &str, key: &str) -> bool {
    if hive(hive_name).is_err() || key.contains('/') {
        return false;
    }
    if hive_name == software.hive
        && key == software.key
        && key.starts_with(&format!("{UNINSTALL}\\"))
        && !key[UNINSTALL.len() + 1..].contains('\\')
    {
        return true;
    }
    let Some(name) = component(&software.name) else {
        return false;
    };
    if key.eq_ignore_ascii_case(&format!("Software\\{name}"))
        && !name.eq_ignore_ascii_case("Microsoft")
        && !name.eq_ignore_ascii_case("Windows")
    {
        return true;
    }
    component(&software.publisher).is_some_and(|publisher| {
        key.eq_ignore_ascii_case(&format!("Software\\{publisher}\\{name}"))
    })
}
pub fn scan(software: &Software, others: &[Software]) -> Vec<Residual> {
    let mut output = vec![];
    let mut directories = vec![];
    if !software.location.is_empty() {
        directories.push((
            PathBuf::from(&software.location),
            "卸载项登记的安装目录",
            false,
        ));
    }
    if let Some(name) = component(&software.name) {
        for variable in ["APPDATA", "LOCALAPPDATA", "ProgramData"] {
            if let Some(root) = std::env::var_os(variable) {
                let root = PathBuf::from(root);
                directories.push((root.join(&name), "目录名与软件名称一致，归属需确认", true));
                if let Some(publisher) = component(&software.publisher) {
                    directories.push((
                        root.join(publisher).join(&name),
                        "发布者和软件名称对应，归属需确认",
                        true,
                    ));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    for (path, evidence, uncertain) in directories {
        if !path.is_dir()
            || !allowed_directory(software, &path)
            || !seen.insert(path.to_string_lossy().to_lowercase())
        {
            continue;
        }
        let shared = others.iter().any(|other| {
            !(other.key == software.key && other.hive == software.hive)
                && !other.location.is_empty()
                && path_contains(&path, Path::new(&other.location))
        });
        if shared {
            continue;
        }
        output.push(Residual {
            software: software.clone(),
            target: ResidualTarget::Directory(path),
            evidence: evidence.into(),
            uncertain,
        });
    }
    if let Some(name) = component(&software.name) {
        for root in shortcut_roots() {
            for path in [
                root.join(format!("{name}.lnk")),
                root.join(&name).join(format!("{name}.lnk")),
            ] {
                if path.is_file() && allowed_file(software, &path) {
                    output.push(Residual {
                        software: software.clone(),
                        target: ResidualTarget::File(path),
                        evidence: "快捷方式名称与软件一致，归属需确认".into(),
                        uncertain: true,
                    });
                }
            }
        }
    }
    let mut keys = vec![(
        software.hive.clone(),
        software.key.clone(),
        "软件自身的卸载登记",
        false,
    )];
    if let Some(name) = component(&software.name) {
        for root in ["HKCU64", "HKCU32", "HKLM64", "HKLM32"] {
            keys.push((
                root.into(),
                format!("Software\\{name}"),
                "注册表名称对应，归属需确认",
                true,
            ));
            if let Some(publisher) = component(&software.publisher) {
                keys.push((
                    root.into(),
                    format!("Software\\{publisher}\\{name}"),
                    "发布者和软件名称对应，归属需确认",
                    true,
                ));
            }
        }
    }
    unsafe {
        for (root, key, evidence, uncertain) in keys {
            if registry_allowed(software, &root, &key) && open(&root, &key, KEY_READ).is_ok() {
                output.push(Residual {
                    software: software.clone(),
                    target: ResidualTarget::Registry { hive: root, key },
                    evidence: evidence.into(),
                    uncertain,
                });
            }
        }
    }
    output
}
pub fn remove(items: &[Residual]) -> Vec<String> {
    let mut errors = vec![];
    let current = enumerate();
    for item in items {
        let result = match &item.target {
            ResidualTarget::Directory(path) => {
                if !allowed_directory(&item.software, path)
                    || current.iter().any(|other| {
                        other.key != item.software.key
                            && !other.location.is_empty()
                            && path_contains(path, Path::new(&other.location))
                    })
                {
                    Err("目录范围校验失败".into())
                } else {
                    match fs::remove_dir_all(path) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(e) => Err(e.to_string()),
                    }
                }
            }
            ResidualTarget::File(path) => {
                if !allowed_file(&item.software, path) {
                    Err("文件范围校验失败".into())
                } else {
                    match fs::remove_file(path) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(e) => Err(e.to_string()),
                    }
                }
            }
            ResidualTarget::Registry { hive: root, key } => {
                if !registry_allowed(&item.software, root, key) {
                    Err("注册表范围校验失败".into())
                } else {
                    unsafe {
                        match key.rsplit_once('\\') {
                            Some((parent, leaf)) => {
                                match open(root, parent, KEY_WRITE | KEY_READ) {
                                    Ok(parent) => {
                                        let code = RegDeleteTreeW(parent.0, wide(leaf).as_ptr());
                                        if code == 0 || code == ERROR_FILE_NOT_FOUND {
                                            Ok(())
                                        } else {
                                            Err(format!("删除注册表失败：{code}"))
                                        }
                                    }
                                    Err(e) if e.ends_with("：2") => Ok(()),
                                    Err(e) => Err(e),
                                }
                            }
                            None => Err("注册表范围无效".into()),
                        }
                    }
                }
            }
        };
        if let Err(e) = result {
            errors.push(format!("{}：{e}", item.label()));
        }
    }
    errors
}
pub fn cleanup_plan(path: &Path) -> Result<()> {
    use std::io::Read;
    if !path.is_absolute() || !no_links(path) {
        return Err("清理计划路径无效".into());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("清理计划过大".into());
    }
    let items: Vec<Residual> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if items.len() > 4096 {
        return Err("清理项目过多".into());
    }
    // The elevated worker repeats directory, junction, shared-app and registry-leaf validation.
    ptools_core::write_json(&path.with_extension("result.json"), &remove(&items))
}
fn cleanup_with_elevation(root: &Path, items: &[Residual]) -> Vec<String> {
    let errors = remove(items);
    if !errors
        .iter()
        .any(|e| e.contains("os error 5") || e.ends_with("：5"))
    {
        return errors;
    }
    let result = (|| -> Result<Vec<String>> {
        let work = root.join("work");
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let plan = work.join(format!("cleanup-{}-{nonce}.json", std::process::id()));
        ptools_core::write_json(&plan, &items)?;
        let execute = (|| -> Result<Vec<String>> {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let code = unsafe {
                shell_wait(
                    &exe.to_string_lossy(),
                    &format!("--cleanup \"{}\"", plan.display()),
                    true,
                )
            }?;
            if code != 0 {
                return Err(format!("管理员清理未完成：{code}"));
            }
            ptools_core::read_json(&plan.with_extension("result.json"))
        })();
        let _ = fs::remove_file(&plan);
        let _ = fs::remove_file(plan.with_extension("result.json"));
        execute
    })();
    match result {
        Ok(errors) => errors,
        Err(e) => {
            let mut errors = errors;
            errors.push(e);
            errors
        }
    }
}
fn uninstall_target(software: &Software) -> Result<(String, String)> {
    if software.msi {
        let product = software.key.rsplit('\\').next().unwrap_or("");
        if product.len() == 38
            && product.starts_with('{')
            && product.ends_with('}')
            && product[1..37]
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-')
        {
            return Ok((
                format!(
                    "{}\\System32\\msiexec.exe",
                    std::env::var("WINDIR").map_err(|e| e.to_string())?
                ),
                format!("/x {product}"),
            ));
        }
    }
    let command = expand(&software.command);
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        let (exe, args) = rest.split_once('"').ok_or("卸载命令引号不完整")?;
        return Ok((exe.into(), args.trim().into()));
    }
    for (i, _) in command.char_indices() {
        let prefix = &command[..i];
        if prefix.to_ascii_lowercase().ends_with(".exe") && Path::new(prefix).is_file() {
            return Ok((prefix.into(), command[i..].trim().into()));
        }
    }
    if command.to_ascii_lowercase().ends_with(".exe") && Path::new(command).is_file() {
        return Ok((command.into(), String::new()));
    }
    let (exe, args) = command.split_once(' ').unwrap_or((command, ""));
    if ["msiexec.exe", "rundll32.exe"]
        .iter()
        .any(|name| exe.eq_ignore_ascii_case(name))
    {
        return Ok((
            format!(
                "{}\\System32\\{exe}",
                std::env::var("WINDIR").map_err(|e| e.to_string())?
            ),
            args.into(),
        ));
    }
    Err("无法明确解析卸载程序路径；请使用打开安装位置检查，或选择强制删除".into())
}
unsafe fn shell_wait(exe: &str, args: &str, elevated: bool) -> Result<u32> {
    let mut info: SHELLEXECUTEINFOW = zeroed();
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    let exe = wide(exe);
    let args = wide(args);
    let verb = wide(if elevated { "runas" } else { "open" });
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    if ShellExecuteExW(&mut info) == 0 {
        return Err(format!("执行被取消或失败：{}", GetLastError()));
    }
    if info.hProcess.is_null() {
        return Err("卸载程序未提供可监控的进程，请完成卸载后手动扫描残留".into());
    }
    WaitForSingleObject(info.hProcess, INFINITE);
    let mut code = 0;
    GetExitCodeProcess(info.hProcess, &mut code);
    CloseHandle(info.hProcess);
    Ok(code)
}

struct Ui {
    hwnd: HWND,
    edit: HWND,
    list: HWND,
    status: HWND,
    software: Vec<Software>,
    rows: Vec<usize>,
    residuals: Vec<Residual>,
    pending: Vec<Software>,
    busy: bool,
    sort: usize,
    descending: bool,
    root: PathBuf,
}
unsafe fn ui(hwnd: HWND) -> *mut Ui {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui
}
unsafe fn cell(list: HWND, row: i32, column: i32, value: &str) {
    let mut text = wide(value);
    let mut item: LVITEMW = zeroed();
    item.iItem = row;
    item.iSubItem = column;
    item.pszText = text.as_mut_ptr();
    SendMessageW(
        list,
        LVM_SETITEMTEXTW,
        row as usize,
        (&item as *const LVITEMW) as isize,
    );
}
unsafe fn columns(list: HWND, residuals: bool) {
    for _ in 0..6 {
        SendMessageW(list, LVM_DELETECOLUMN, 0, 0);
    }
    let labels: Vec<(&str, i32)> = if residuals {
        vec![("待删除位置", 470), ("软件", 170), ("归属依据", 320)]
    } else {
        vec![
            ("软件名称", 260),
            ("版本", 120),
            ("发布者", 180),
            ("大小", 90),
            ("安装日期", 110),
            ("安装位置", 310),
        ]
    };
    for (index, (label, width)) in labels.iter().enumerate() {
        let mut text = wide(label);
        let mut column: LVCOLUMNW = zeroed();
        column.mask = LVCF_TEXT | LVCF_WIDTH;
        column.cx = *width;
        column.pszText = text.as_mut_ptr();
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            index,
            (&column as *const LVCOLUMNW) as isize,
        );
    }
}
unsafe fn populate(state: &mut Ui) {
    SendMessageW(state.list, WM_SETREDRAW, 0, 0);
    SendMessageW(state.list, LVM_DELETEALLITEMS, 0, 0);
    if !state.residuals.is_empty() {
        columns(state.list, true);
        for (index, item) in state.residuals.iter().enumerate() {
            let mut label = wide(&item.label());
            let mut row: LVITEMW = zeroed();
            row.mask = LVIF_TEXT;
            row.iItem = index as i32;
            row.pszText = label.as_mut_ptr();
            SendMessageW(
                state.list,
                LVM_INSERTITEMW,
                0,
                (&row as *const LVITEMW) as isize,
            );
            cell(state.list, index as i32, 1, &item.software.name);
            cell(
                state.list,
                index as i32,
                2,
                &format!(
                    "{}{}",
                    if item.uncertain { "待确认：" } else { "" },
                    item.evidence
                ),
            );
        }
        SetWindowTextW(
            state.status,
            wide("残留默认未勾选。检查位置和依据后，勾选需要删除的项目，再点“清理已选”。").as_ptr(),
        );
    } else {
        columns(state.list, false);
        let query = text(state.edit).to_lowercase();
        state.rows = (0..state.software.len())
            .filter(|i| {
                let software = &state.software[*i];
                format!("{} {}", software.name, software.publisher)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        state.rows.sort_by(|a, b| {
            let a = &state.software[*a];
            let b = &state.software[*b];
            let order = match state.sort {
                1 => a.version.cmp(&b.version),
                2 => a.publisher.to_lowercase().cmp(&b.publisher.to_lowercase()),
                3 => a.size_kb.cmp(&b.size_kb),
                4 => a.date.cmp(&b.date),
                5 => a.location.cmp(&b.location),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            if state.descending {
                order.reverse()
            } else {
                order
            }
        });
        for (row, index) in state.rows.iter().enumerate() {
            let software = &state.software[*index];
            let mut label = wide(&software.name);
            let mut item: LVITEMW = zeroed();
            item.mask = LVIF_TEXT;
            item.iItem = row as i32;
            item.pszText = label.as_mut_ptr();
            SendMessageW(
                state.list,
                LVM_INSERTITEMW,
                0,
                (&item as *const LVITEMW) as isize,
            );
            for (column, value) in [
                (1, software.version.clone()),
                (2, software.publisher.clone()),
                (
                    3,
                    if software.size_kb == 0 {
                        "—".into()
                    } else {
                        format!("{:.1} MB", software.size_kb as f64 / 1024.0)
                    },
                ),
                (4, software.date.clone()),
                (5, software.location.clone()),
            ] {
                cell(state.list, row as i32, column, &value);
            }
        }
        SetWindowTextW(
            state.status,
            wide(&format!(
                "{} 个桌面软件 · 勾选可批量卸载 · 点击列标题排序",
                state.rows.len()
            ))
            .as_ptr(),
        );
    }
    SendMessageW(state.list, WM_SETREDRAW, 1, 0);
    InvalidateRect(state.list, null(), 1);
}
unsafe fn selected(state: &Ui) -> Vec<usize> {
    let count = SendMessageW(state.list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
    let mut selected: Vec<usize> = (0..count)
        .filter(|i| {
            SendMessageW(
                state.list,
                LVM_GETITEMSTATE,
                *i,
                LVIS_STATEIMAGEMASK as isize,
            ) >> 12
                == 2
        })
        .collect();
    if selected.is_empty() {
        let row = SendMessageW(
            state.list,
            LVM_GETNEXTITEM,
            usize::MAX,
            LVNI_SELECTED as isize,
        );
        if row >= 0 {
            selected.push(row as usize);
        }
    }
    selected
}
enum Work {
    Uninstalled(Vec<Software>, Vec<String>),
    Deleted(Vec<String>),
}
unsafe fn work(hwnd: HWND, state: &mut Ui, kind: usize) -> Result<()> {
    if state.busy {
        return Err("正在执行操作，请等待完成".into());
    }
    if kind == 14 {
        if state.residuals.is_empty() {
            return Err("请先扫描残留".into());
        }
        // Cleanup requires explicit checkboxes; a highlighted row alone never authorizes deletion.
        let items: Vec<_> = (0..state.residuals.len())
            .filter(|i| {
                SendMessageW(
                    state.list,
                    LVM_GETITEMSTATE,
                    *i,
                    LVIS_STATEIMAGEMASK as isize,
                ) >> 12
                    == 2
            })
            .map(|i| state.residuals[i].clone())
            .collect();
        if items.is_empty() {
            return Err("请勾选需要删除的残留项目".into());
        }
        if !confirm(
            hwnd,
            &format!("删除已勾选的 {} 个项目？此操作不保留备份。", items.len()),
        ) {
            return Ok(());
        }
        state.busy = true;
        let window = hwnd as usize;
        let root = state.root.clone();
        std::thread::spawn(move || {
            let errors = cleanup_with_elevation(&root, &items);
            let result = Box::into_raw(Box::new(Work::Deleted(errors)));
            unsafe {
                if PostMessageW(window as HWND, WM_DONE, 0, result as isize) == 0 {
                    drop(Box::from_raw(result));
                }
            }
        });
        return Ok(());
    }
    let selection = selected(state);
    if selection.is_empty() {
        return Err("请先选择软件".into());
    }
    let software: Vec<_> = selection
        .into_iter()
        .filter_map(|i| state.rows.get(i))
        .map(|i| state.software[*i].clone())
        .collect();
    if software.is_empty() {
        return Err("请返回软件列表后选择软件".into());
    }
    if kind == 12 {
        for software in &software {
            if !software.location.is_empty() {
                ShellExecuteW(
                    hwnd,
                    wide("open").as_ptr(),
                    wide(&software.location).as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        return Ok(());
    }
    if kind == 11 || kind == 13 {
        if kind == 13
            && !confirm(
                hwnd,
                "强制删除不会运行软件自带的卸载程序。下一步只列出可定位项目，勾选并确认后才会删除。继续？",
            )
        {
            return Ok(());
        }
        state.pending = software.clone();
        state.residuals = software
            .iter()
            .flat_map(|software| scan(software, &state.software))
            .collect();
        if state.residuals.is_empty() {
            alert(hwnd, "没有找到可明确定位的残留项目");
        }
        populate(state);
        return Ok(());
    }
    if !confirm(
        hwnd,
        &format!(
            "依次卸载以下软件？\n\n{}\n\n将调用各软件自身的卸载程序。",
            software
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        ),
    ) {
        return Ok(());
    }
    state.busy = true;
    SetWindowTextW(
        state.status,
        wide("正在卸载，完成后扫描残留；请在软件自身的卸载界面中操作。").as_ptr(),
    );
    let window = hwnd as usize;
    std::thread::spawn(move || {
        let mut removed = vec![];
        let mut errors = vec![];
        for software in software {
            match uninstall_target(&software)
                .and_then(|(exe, args)| unsafe { shell_wait(&exe, &args, false) })
            {
                Ok(0 | 3010) => removed.push(software),
                Ok(1602) => errors.push(format!("{}：卸载已取消", software.name)),
                Ok(code) => errors.push(format!(
                    "{}：卸载程序返回 {code}，未自动清理",
                    software.name
                )),
                Err(e) => errors.push(format!("{}：{e}", software.name)),
            }
        }
        let result = Box::into_raw(Box::new(Work::Uninstalled(removed, errors)));
        unsafe {
            if PostMessageW(window as HWND, WM_DONE, 0, result as isize) == 0 {
                drop(Box::from_raw(result));
            }
        }
    });
    Ok(())
}

pub fn run(initial: Option<Invocation>, smoke: bool) -> Result<()> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        InitCommonControlsEx(&controls);
        let module = GetModuleHandleW(null());
        let class = wide("ptools.uninstaller");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as _,
            ..zeroed()
        };
        RegisterClassW(&wc);
        let mut state = Box::new(Ui {
            hwnd: null_mut(),
            edit: null_mut(),
            list: null_mut(),
            status: null_mut(),
            software: enumerate(),
            rows: vec![],
            residuals: vec![],
            pending: vec![],
            busy: false,
            sort: 0,
            descending: false,
            root: PathBuf::new(),
        });
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("ptools 软件卸载").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1100,
            720,
            null_mut(),
            null_mut(),
            module,
            (&mut *state as *mut Ui).cast(),
        );
        if hwnd.is_null() {
            return Err("无法打开卸载界面".into());
        }
        state.hwnd = hwnd;
        state.edit = child(
            hwnd,
            "EDIT",
            "",
            WS_BORDER | ES_AUTOHSCROLL as u32,
            1,
            [12, 12, 500, 30],
        );
        for (i, label) in [
            "卸载已选",
            "扫描残留",
            "安装位置",
            "强制删除",
            "清理已选",
            "返回/刷新",
        ]
        .iter()
        .enumerate()
        {
            child(
                hwnd,
                "BUTTON",
                label,
                0,
                10 + i,
                [12 + i as i32 * 148, 54, 138, 32],
            );
        }
        state.list = child(
            hwnd,
            "SysListView32",
            "",
            LVS_REPORT | LVS_SHOWSELALWAYS | WS_BORDER,
            2,
            [12, 100, 1050, 520],
        );
        SendMessageW(
            state.list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        state.status = child(hwnd, "STATIC", "", 0, 3, [12, 634, 1050, 30]);
        populate(&mut state);
        if let Some(request) = initial {
            PostMessageW(
                hwnd,
                WM_INVOKE,
                0,
                Box::into_raw(Box::new(request)) as isize,
            );
        } else {
            read_invocations(hwnd);
        }
        if smoke {
            SetTimer(hwnd, 99, 1200, None);
        }
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = ui(hwnd);
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, w, l);
    }
    let state = &mut *pointer;
    match message {
        WM_INVOKE => {
            if l == 0 {
                if !state.busy {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            let request = Box::from_raw(l as *mut Invocation);
            state.root = request.data_dir;
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            return 0;
        }
        WM_COMMAND => {
            let id = w & 0xffff;
            let code = w >> 16;
            if id == 1
                && code == EN_CHANGE as usize
                && !state.list.is_null()
                && state.residuals.is_empty()
            {
                populate(state);
                return 0;
            }
            if (10..=15).contains(&id) {
                if id == 15 {
                    if !state.busy {
                        state.residuals.clear();
                        state.pending.clear();
                        state.software = enumerate();
                        populate(state);
                    }
                } else if let Err(e) = work(hwnd, state, id) {
                    alert(hwnd, &e);
                }
                return 0;
            }
        }
        WM_NOTIFY => {
            let header = &*(l as *const NMHDR);
            if header.code == LVN_COLUMNCLICK && state.residuals.is_empty() {
                let info = &*(l as *const NMLISTVIEW);
                state.descending = state.sort == info.iSubItem as usize && !state.descending;
                state.sort = info.iSubItem as usize;
                populate(state);
                return 0;
            }
        }
        WM_DONE => {
            let result = Box::from_raw(l as *mut Work);
            state.busy = false;
            match *result {
                Work::Uninstalled(removed, errors) => {
                    state.software = enumerate();
                    state.pending = removed;
                    state.residuals = state
                        .pending
                        .iter()
                        .flat_map(|software| scan(software, &state.software))
                        .collect();
                    populate(state);
                    if !errors.is_empty() {
                        alert(hwnd, &errors.join("\n"));
                    }
                }
                Work::Deleted(errors) => {
                    state.software = enumerate();
                    state.residuals = state
                        .pending
                        .iter()
                        .flat_map(|software| scan(software, &state.software))
                        .collect();
                    populate(state);
                    if errors.is_empty() {
                        alert(hwnd, "已清理勾选的残留");
                    } else {
                        alert(hwnd, &format!("部分项目未能删除：\n{}", errors.join("\n")));
                    }
                }
            }
            return 0;
        }
        WM_SIZE => {
            if !state.list.is_null() {
                let mut r = RECT::default();
                GetClientRect(hwnd, &mut r);
                MoveWindow(
                    state.list,
                    12,
                    100,
                    (r.right - 24).max(200),
                    (r.bottom - 150).max(100),
                    1,
                );
                MoveWindow(
                    state.status,
                    12,
                    r.bottom - 38,
                    (r.right - 24).max(200),
                    30,
                    1,
                );
            }
            return 0;
        }
        WM_TIMER if w == 99 => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_CLOSE => {
            if state.busy {
                alert(hwnd, "卸载或清理正在进行，请等待完成后关闭");
            } else {
                DestroyWindow(hwnd);
            }
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, w, l)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn software() -> Software {
        Software {
            hive: "HKCU64".into(),
            key: format!("{UNINSTALL}\\example"),
            name: "Example App".into(),
            publisher: "Example".into(),
            version: "1".into(),
            size_kb: 0,
            date: String::new(),
            location: String::new(),
            command: String::new(),
            msi: false,
        }
    }
    #[test]
    fn cleanup_rejects_shared_roots_and_registry_parent_keys() {
        let software = software();
        for variable in [
            "WINDIR",
            "ProgramFiles",
            "APPDATA",
            "LOCALAPPDATA",
            "USERPROFILE",
        ] {
            if let Some(path) = std::env::var_os(variable) {
                let path = PathBuf::from(path);
                assert!(protected(&path));
                assert!(!allowed_directory(&software, &path));
            }
        }
        assert!(!registry_allowed(
            &software,
            "HKCU64",
            "Software\\Microsoft"
        ));
        assert!(!registry_allowed(&software, "HKCU64", UNINSTALL));
        assert!(!registry_allowed(&software, "HKCU64", "Software\\Other"));
        assert!(registry_allowed(&software, "HKCU64", &software.key));
    }
    #[test]
    fn cleans_only_explicit_fixture_and_keeps_sibling() {
        let root = std::env::temp_dir().join(format!(
            "ptools-cleanup-{}-{}",
            std::process::id(),
            ptools_core::now()
        ));
        fs::create_dir_all(root.join("app")).unwrap();
        fs::write(root.join("app/a.txt"), "fixture").unwrap();
        fs::write(root.join("keep.txt"), "keep").unwrap();
        fs::create_dir(root.join("outside")).unwrap();
        fs::write(root.join("outside/keep.txt"), "keep").unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("app").join("linked"))
            .arg(root.join("outside"))
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        let mut software = software();
        software.location = root.join("app").display().to_string();
        let item = Residual {
            software,
            target: ResidualTarget::Directory(root.join("app")),
            evidence: "fixture".into(),
            uncertain: false,
        };
        assert!(remove(&[item]).is_empty());
        assert!(root.join("keep.txt").exists());
        assert!(root.join("outside/keep.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
