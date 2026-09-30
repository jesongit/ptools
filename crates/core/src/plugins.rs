use crate::*;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_RESPONSE: u64 = 8 * 1024 * 1024;
const MAX_PACKAGE: u64 = 128 * 1024 * 1024;

#[cfg(windows)]
pub struct PluginJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl PluginJob {
    pub fn attach(child: &std::process::Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for PluginJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub manifest: Manifest,
    pub directory: PathBuf,
}

fn safe_relative(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("必须使用插件目录内的相对路径：{value}"));
    }
    Ok(path.to_owned())
}

fn reject_plugin_link(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if reparse || metadata.file_type().is_symlink() {
        return Err(format!(
            "插件文件不允许符号链接或目录连接：{}",
            path.display()
        ));
    }
    Ok(())
}

fn plugin_file(directory: &Path, relative: &str) -> Result<PathBuf> {
    let relative = safe_relative(relative)?;
    let mut path = directory.to_owned();
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    reject_plugin_link(&path, &metadata)?;
    if !metadata.is_dir() {
        return Err("插件路径必须是目录".into());
    }
    // Validate within the logical filesystem namespace. Under MSIX AppData
    // virtualization, canonicalizing a directory and its files can yield
    // different physical roots. Relative components plus a no-links check at
    // every step retain the boundary without comparing those physical strings.
    for component in relative.components() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("无法读取插件文件 {}：{e}", path.display()))?;
        reject_plugin_link(&path, &metadata)?;
    }
    if !path.is_file() {
        return Err(format!("插件入口必须是文件：{}", path.display()));
    }
    Ok(path)
}

pub fn load_plugin(directory: &Path) -> Result<Plugin> {
    let file = plugin_file(directory, "plugin.json")?;
    let size = fs::metadata(&file)
        .map_err(|e| format!("{}: {e}", file.display()))?
        .len();
    if size > MAX_RESPONSE {
        return Err("插件清单过大".into());
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    if !matches!(manifest.schema_version, 1 | 2) {
        return Err("不支持的插件清单版本".into());
    }
    if manifest.id.is_empty()
        || manifest.id.len() > 64
        || !manifest
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err("插件 ID 只能包含小写字母、数字、短横线和下划线".into());
    }
    if manifest.name.trim().is_empty() || manifest.name.len() > 256 || manifest.version.len() > 64 {
        return Err("插件名称或版本无效".into());
    }
    match manifest.runtime {
        Runtime::Executable | Runtime::Interactive => {
            let exe = safe_relative(
                manifest
                    .executable
                    .as_deref()
                    .ok_or("缺少 executable 字段")?,
            )?;
            if exe.extension().and_then(|s| s.to_str()) != Some("exe") {
                return Err("原生插件入口必须是 .exe".into());
            }
            plugin_file(directory, exe.to_str().ok_or("无效插件入口")?)?;
        }
        Runtime::Config => {
            validate_entries(&manifest.entries)?;
        }
    }
    if manifest.runtime == Runtime::Interactive {
        if manifest.schema_version != 2 || manifest.actions.is_empty() {
            return Err("交互插件需要 schema_version 2 和 actions".into());
        }
        validate_entries(&action_entries(&manifest))?;
    } else if !manifest.actions.is_empty() {
        return Err("只有交互插件可提供动作".into());
    }
    Ok(Plugin {
        manifest,
        directory: directory.to_owned(),
    })
}

pub fn discover_plugins(root: &Path) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    if let Ok(dirs) = fs::read_dir(root) {
        for dir in dirs.flatten() {
            if dir.file_name().to_string_lossy().starts_with('.') || !dir.path().is_dir() {
                continue;
            }
            match load_plugin(&dir.path()) {
                Ok(plugin) if dir.file_name().to_str() == Some(plugin.manifest.id.as_str()) => {
                    plugins.push(plugin)
                }
                Ok(_) => errors.push(format!(
                    "{}: 目录名必须与插件 ID 一致",
                    dir.path().display()
                )),
                Err(e) => errors.push(format!("{}: {e}", dir.path().display())),
            }
        }
    }
    plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    (plugins, errors)
}

fn validate_entries(entries: &[Entry]) -> Result<()> {
    if entries.len() > 20000 {
        return Err("单个插件最多提供 20000 个索引条目".into());
    }
    let mut ids = BTreeSet::new();
    for entry in entries {
        validate_entry(entry)?;
        if !ids.insert(&entry.id) {
            return Err(format!("重复的条目 ID：{}", entry.id));
        }
    }
    Ok(())
}

pub fn run_index(plugin: &Plugin, timeout: Duration) -> Result<IndexResponse> {
    if plugin.manifest.runtime == Runtime::Interactive {
        return Ok(IndexResponse {
            protocol_version: 1,
            entries: action_entries(&plugin.manifest),
            warnings: vec![],
        });
    }
    if plugin.manifest.runtime == Runtime::Config {
        return Ok(IndexResponse {
            protocol_version: 1,
            entries: plugin.manifest.entries.clone(),
            warnings: vec![],
        });
    }
    let exe = plugin_file(
        &plugin.directory,
        plugin
            .manifest
            .executable
            .as_deref()
            .ok_or("缺少插件入口")?,
    )?;
    let mut command = Command::new(exe);
    command
        .current_dir(&plugin.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 {}：{e}", plugin.manifest.name))?;
    #[cfg(windows)]
    let _job = match PluginJob::attach(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法管理插件进程：{e}"));
        }
    };
    let request = serde_json::to_vec(&IndexRequest {
        protocol_version: 1,
        operation: "index".into(),
    })
    .unwrap();
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin
            .write_all(&request)
            .and_then(|_| stdin.write_all(b"\n"))
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.to_string());
    }
    let stdout = child.stdout.take().ok_or("无法读取插件输出")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    let started = Instant::now();
    let mut bytes = None;
    loop {
        if bytes.is_none()
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(output) if output.len() as u64 <= MAX_RESPONSE => bytes = Some(output),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("插件输出异常或超过 8 MB".into());
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(format!("{} 运行失败：{status}", plugin.manifest.name));
                }
                if let Some(bytes) = bytes {
                    return parse_response(&bytes);
                }
                let remaining = timeout.saturating_sub(started.elapsed());
                let bytes = rx
                    .recv_timeout(remaining)
                    .map_err(|_| "等待插件输出超时")??;
                if bytes.len() as u64 > MAX_RESPONSE {
                    return Err("插件输出超过 8 MB".into());
                }
                return parse_response(&bytes);
            }
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} 刷新超时", plugin.manifest.name));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn action_entries(manifest: &Manifest) -> Vec<Entry> {
    manifest
        .actions
        .iter()
        .map(|action| Entry {
            id: action.id.clone(),
            title: action.title.clone(),
            subtitle: action.subtitle.clone(),
            keywords: action.keywords.clone(),
            target: Target::Plugin {
                action: action.id.clone(),
            },
        })
        .collect()
}

pub fn interactive_executable(plugin: &Plugin, action: &str) -> Result<PathBuf> {
    if plugin.manifest.runtime != Runtime::Interactive
        || !plugin.manifest.actions.iter().any(|a| a.id == action)
    {
        return Err("插件未声明这个动作".into());
    }
    plugin_file(
        &plugin.directory,
        plugin
            .manifest
            .executable
            .as_deref()
            .ok_or("缺少插件入口")?,
    )
}

fn parse_response(bytes: &[u8]) -> Result<IndexResponse> {
    let response: IndexResponse =
        serde_json::from_slice(bytes).map_err(|e| format!("插件返回的 JSON 无效：{e}"))?;
    if response.protocol_version != 1 {
        return Err("插件协议版本不兼容".into());
    }
    validate_entries(&response.entries)?;
    Ok(response)
}

pub fn refresh(paths: &Paths, settings: &Settings, old: &Cache) -> (Cache, Vec<String>) {
    let (plugins, mut errors) = discover_plugins(&paths.plugins);
    let mut cache = Cache::default();
    for plugin in plugins {
        let id = &plugin.manifest.id;
        if settings.disabled_plugins.contains(id) {
            continue;
        }
        match run_index(&plugin, Duration::from_secs(30)) {
            Ok(response) => {
                errors.extend(
                    response
                        .warnings
                        .into_iter()
                        .map(|e| format!("{}: {e}", plugin.manifest.name)),
                );
                cache.plugins.insert(id.clone(), response.entries);
            }
            Err(e) => {
                errors.push(e);
                if let Some(entries) = old.plugins.get(id) {
                    cache.plugins.insert(id.clone(), entries.clone());
                }
            }
        }
    }
    cache.updated_at = now();
    (cache, errors)
}

fn copy_directory(source: &Path, target: &Path, total: &mut u64, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err("插件目录层级过深".into());
    }
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let meta = fs::symlink_metadata(item.path()).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("插件包不允许符号链接或目录连接".into());
            }
        }
        if meta.file_type().is_symlink() {
            return Err("插件包不允许符号链接".into());
        }
        let dest = target.join(item.file_name());
        if meta.is_dir() {
            copy_directory(&item.path(), &dest, total, depth + 1)?;
        } else if meta.is_file() {
            *total += meta.len();
            if *total > MAX_PACKAGE {
                return Err("插件包超过 128 MB".into());
            }
            fs::copy(item.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn install_plugin(source: &Path, root: &Path) -> Result<Manifest> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let stage = root.join(format!(".install-{}-{}", std::process::id(), now()));
    if stage.exists() {
        return Err("安装暂存目录已存在，请重试".into());
    }
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        if source.is_dir() || source.file_name().and_then(|s| s.to_str()) == Some("plugin.json") {
            let source = if source.is_dir() {
                source
            } else {
                source.parent().ok_or("无效路径")?
            };
            let canonical = source.canonicalize().map_err(|e| e.to_string())?;
            let dest = stage.canonicalize().map_err(|e| e.to_string())?;
            if dest.starts_with(&canonical) {
                return Err("不能从包含插件安装目录的父目录安装".into());
            }
            copy_directory(source, &stage, &mut 0, 0)?;
        } else {
            let file = fs::File::open(source).map_err(|e| e.to_string())?;
            let mut archive =
                zip::ZipArchive::new(file).map_err(|e| format!("无效的插件包：{e}"))?;
            if archive.len() > 4096 {
                return Err("插件包文件数量过多".into());
            }
            let mut total = 0;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                total += entry.size();
                if total > MAX_PACKAGE {
                    return Err("插件包解压后超过 128 MB".into());
                }
                if entry.is_symlink() {
                    return Err("插件包不允许符号链接".into());
                }
                let name = entry.enclosed_name().ok_or("插件包包含越界路径")?;
                let path = stage.join(safe_relative(
                    name.to_str()
                        .ok_or("无效文件名")?
                        .trim_end_matches(['/', '\\']),
                )?);
                if entry.is_dir() {
                    fs::create_dir_all(path).map_err(|e| e.to_string())?;
                } else {
                    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                    let mut out = fs::File::create(path).map_err(|e| e.to_string())?;
                    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
                }
            }
        }
        let plugin = load_plugin(&stage)?;
        let destination = root.join(&plugin.manifest.id);
        if destination.exists() {
            return Err("插件已安装；请先卸载旧版本，再安装新版本".into());
        }
        fs::rename(&stage, destination).map_err(|e| e.to_string())?;
        Ok(plugin.manifest)
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

pub fn uninstall_plugin(root: &Path, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err("无效插件 ID".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = root.join(id).canonicalize().map_err(|e| e.to_string())?;
    if target.parent() != Some(root.as_path()) {
        return Err("拒绝删除插件目录之外的路径".into());
    }
    fs::remove_dir_all(target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ptools-test-{}-{}-{name}",
            std::process::id(),
            now()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn install_validate_refresh_uninstall() {
        let base = temp("install");
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("plugin.json"), r#"{"schema_version":1,"id":"test","name":"测试","version":"1","runtime":"config","entries":[{"id":"web","title":"网页","target":{"kind":"url","url":"https://example.com"}}]}"#).unwrap();
        let root = base.join("plugins");
        assert_eq!(install_plugin(&source, &root).unwrap().id, "test");
        assert!(install_plugin(&source, &root).is_err());
        let plugin = load_plugin(&root.join("test")).unwrap();
        assert_eq!(
            run_index(&plugin, Duration::from_secs(1))
                .unwrap()
                .entries
                .len(),
            1
        );
        assert!(uninstall_plugin(&root, "../source").is_err());
        uninstall_plugin(&root, "test").unwrap();
        assert!(source.exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn reject_duplicate_and_invalid_targets() {
        let entry = Entry {
            id: "x".into(),
            title: "x".into(),
            subtitle: String::new(),
            keywords: vec![],
            target: Target::Url {
                url: "file:///etc/passwd".into(),
            },
        };
        assert!(validate_entry(&entry).is_err());
        assert!(safe_relative("../bad.exe").is_err());
        assert!(safe_relative("C:\\bad.exe").is_err());
        assert!(parse_response(br#"{"protocol_version":2,"entries":[]}"#).is_err());
        let valid = Entry {
            target: Target::Url {
                url: "https://example.com".into(),
            },
            ..entry
        };
        assert!(validate_entries(&[valid.clone(), valid]).is_err());
    }

    #[test]
    fn reject_zip_path_traversal_without_partial_install() {
        let base = temp("zip-traversal");
        let archive = base.join("bad.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("../escaped.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"unexpected").unwrap();
        zip.finish().unwrap();
        let root = base.join("plugins");
        assert!(install_plugin(&archive, &root).is_err());
        assert!(!base.join("escaped.txt").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn disabled_plugin_is_not_indexed() {
        let base = temp("disabled");
        let paths = Paths::resolve(&base, true, Some(base.join("data"))).unwrap();
        let directory = paths.plugins.join("config");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("plugin.json"), r#"{"schema_version":1,"id":"config","name":"test","version":"1","runtime":"config","entries":[{"id":"a","title":"A","target":{"kind":"url","url":"https://example.com"}}]}"#).unwrap();
        let mut settings = Settings::default();
        let (cache, errors) = refresh(&paths, &settings, &Cache::default());
        assert!(errors.is_empty());
        assert_eq!(cache.plugins["config"].len(), 1);
        settings.disabled_plugins.insert("config".into());
        let (cache, errors) = refresh(&paths, &settings, &cache);
        assert!(errors.is_empty());
        assert!(cache.plugins.is_empty());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn executable_paths_stay_inside_plugin() {
        let base = temp("executable-paths");
        let plugin = base.join("plugin");
        fs::create_dir_all(plugin.join("bin")).unwrap();
        fs::write(plugin.join("bin/tool.exe"), b"fixture").unwrap();
        assert_eq!(
            plugin_file(&plugin, "bin/tool.exe").unwrap(),
            plugin.join("bin/tool.exe")
        );
        for path in [
            "../outside.exe",
            "bin/../../outside.exe",
            "C:\\outside.exe",
            "bin",
            "missing.exe",
        ] {
            assert!(plugin_file(&plugin, path).is_err(), "accepted {path}");
        }
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn interactive_index_never_executes_and_dispatch_requires_declared_action() {
        let base = temp("interactive");
        // Deliberately not an executable: indexing must come entirely from the manifest.
        fs::write(base.join("tool.exe"), b"not a PE executable").unwrap();
        let mut manifest = serde_json::json!({"schema_version":2,"id":"test","name":"test","version":"1","runtime":"interactive","executable":"tool.exe","actions":[{"id":"open","title":"Open","hotkey":"Ctrl+1"}]});
        let file = base.join("plugin.json");
        write_json(&file, &manifest).unwrap();
        let plugin = load_plugin(&base).unwrap();
        let indexed = run_index(&plugin, Duration::from_millis(1)).unwrap();
        assert_eq!(indexed.entries.len(), 1);
        assert!(
            matches!(&indexed.entries[0].target, Target::Plugin { action } if action == "open")
        );
        assert!(interactive_executable(&plugin, "open").is_ok());
        assert!(interactive_executable(&plugin, "undeclared").is_err());
        manifest["actions"][0]["id"] = "../bad".into();
        write_json(&file, &manifest).unwrap();
        assert!(load_plugin(&base).is_err());
        manifest["actions"][0]["id"] = "open".into();
        manifest["schema_version"] = 1.into();
        write_json(&file, &manifest).unwrap();
        assert!(load_plugin(&base).is_err());
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn executable_loads_from_roaming_appdata() {
        // Exercise the same namespace as normal GUI startup, including AppData
        // virtualization when the test is launched by a packaged desktop app.
        let base = PathBuf::from(std::env::var_os("APPDATA").unwrap()).join(format!(
            "ptools-test-{}-{}-roaming",
            std::process::id(),
            now()
        ));
        fs::create_dir(&base).unwrap();
        fs::write(base.join("tool.exe"), b"fixture").unwrap();
        fs::write(base.join("plugin.json"), r#"{"schema_version":1,"id":"test","name":"test","version":"1","runtime":"executable","executable":"tool.exe"}"#).unwrap();
        let result = load_plugin(&base);
        fs::remove_dir_all(&base).unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn reject_executable_through_directory_junction() {
        let base = temp("junction");
        let plugin = base.join("plugin");
        let outside = base.join("outside");
        fs::create_dir_all(&plugin).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("tool.exe"), b"outside").unwrap();
        let link = plugin.join("linked");
        let output = Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = plugin_file(&plugin, "linked/tool.exe");
        fs::remove_dir(&link).unwrap();
        assert_eq!(fs::read(outside.join("tool.exe")).unwrap(), b"outside");
        fs::remove_dir_all(base).unwrap();
        assert!(result.unwrap_err().contains("目录连接"));
    }
}
