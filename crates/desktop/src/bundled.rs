use ptools_core::*;
use std::{fs, path::Path};

// Only release versions of first-party bundles participate in automatic upgrades.
// Unknown/custom versions are left to the user's plugin management flow.
fn release_version(value: &str) -> Option<[u64; 3]> {
    let parts = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    parts.try_into().ok()
}

fn upgrade(source: &Path, paths: &Paths, id: &str) -> Result<()> {
    let destination = paths.plugins.join(id);
    let backup = paths.plugins.join(format!(".bundled-backup-{id}"));
    let staging = paths.plugins.join(format!(
        ".bundled-stage-{id}-{}-{}",
        std::process::id(),
        now()
    ));
    // Fully copy and validate before touching the installed plugin.
    let result = (|| {
        install_plugin(source, &staging)?;
        fs::rename(&destination, &backup)
            .map_err(|e| format!("无法更新 {id}，请关闭正在运行的工具后重试：{e}"))?;
        if let Err(error) = fs::rename(staging.join(id), &destination) {
            fs::rename(&backup, &destination)
                .map_err(|e| format!("{id} 更新失败：{error}；恢复失败：{e}"))?;
            return Err(format!("{id} 更新失败，已恢复原插件：{error}"));
        }
        // A leftover backup is harmless and is cleaned on the next startup.
        let _ = fs::remove_dir_all(&backup);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Call only when no host instance is using this data directory. Configuration,
/// cache and plugin-data are separate from the replaceable plugin programs.
pub fn sync(exe_dir: &Path, paths: &Paths) -> Result<()> {
    retire_uninstaller(paths)?;
    for id in ["applications", "capture"] {
        let source = exe_dir.join("plugins").join(id);
        if !source.exists() {
            continue;
        }
        let bundled = load_plugin(&source)?;
        if bundled.manifest.id != id {
            return Err(format!("随包插件 {id} 的清单 ID 不匹配"));
        }
        let destination = paths.plugins.join(id);
        let backup = paths.plugins.join(format!(".bundled-backup-{id}"));
        if backup.exists() {
            load_plugin(&backup)?;
            if !destination.exists() {
                fs::rename(&backup, &destination).map_err(|e| e.to_string())?;
            } else {
                load_plugin(&destination)?;
                fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
            }
        }
        let marker = paths.root.join(if id == "applications" {
            "initialized".to_owned()
        } else {
            format!("initialized-{id}")
        });
        if destination.exists() {
            let installed = load_plugin(&destination)?;
            if installed.manifest.id != id {
                return Err(format!("已安装插件 {id} 的清单 ID 不匹配"));
            }
            if let (Some(incoming), Some(current)) = (
                release_version(&bundled.manifest.version),
                release_version(&installed.manifest.version),
            ) && incoming > current
            {
                upgrade(&source, paths, id)?;
            }
        } else if !marker.exists() {
            install_plugin(&source, &paths.plugins)?;
        }
        if !marker.exists() {
            fs::write(marker, b"1").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn retire_uninstaller(paths: &Paths) -> Result<()> {
    let directory = paths.plugins.join("uninstaller");
    if !directory.exists() {
        return Ok(());
    }
    let Ok(manifest) = load_plugin_manifest(&directory) else {
        return Ok(());
    };
    let legacy = manifest.id == "uninstaller"
        && manifest.runtime == Runtime::Interactive
        && manifest.executable.as_deref() == Some("ptools-uninstaller.exe")
        && release_version(&manifest.version).is_some_and(|version| version <= [0, 1, 1]);
    if legacy {
        // Clear cached actions before deletion, so failures can retry on next startup.
        let mut cache: Cache = read_json(&paths.cache())?;
        if cache.plugins.remove("uninstaller").is_some() {
            write_json(&paths.cache(), &cache)?;
        }
        // Manifest loading validates the logical directory and rejects reparse points.
        // Keep user data and preferences; remove only the retired first-party program.
        fs::remove_dir_all(&directory).map_err(|error| format!("无法移除旧卸载插件：{error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (std::path::PathBuf, Paths) {
        let root = std::env::temp_dir().join(format!(
            "ptools-bundled-{}-{}-{name}",
            std::process::id(),
            now()
        ));
        let paths = Paths::resolve(&root, false, Some(root.join("data"))).unwrap();
        (root, paths)
    }
    fn bundle(root: &Path, version: &str, payload: &[u8]) {
        let directory = root.join("plugins/capture");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("plugin.json"), serde_json::to_vec(&serde_json::json!({
            "schema_version":2, "id":"capture", "name":"截图", "version":version,
            "runtime":"interactive", "executable":"tool.exe", "actions":[{"id":"open","title":"截图"}]
        })).unwrap()).unwrap();
        fs::write(directory.join("tool.exe"), payload).unwrap();
    }

    #[test]
    fn upgrades_old_install_preserving_data_and_disabled_state() {
        let (root, paths) = fixture("upgrade");
        bundle(&root, "0.1.0", b"old tool");
        sync(&root, &paths).unwrap();
        let data = paths.root.join("plugin-data/capture");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("settings.json"), b"user preferences").unwrap();
        fs::write(data.join("history.json"), b"user history").unwrap();
        let mut settings = Settings::default();
        settings.disabled_plugins.insert("capture".into());
        settings.usage.insert(
            "capture:open".into(),
            Usage {
                count: 7,
                last_used: 12,
            },
        );
        write_json(&paths.settings(), &settings).unwrap();
        let before = fs::read(paths.settings()).unwrap();
        bundle(&root, "0.1.1", b"new themed tool");
        sync(&root, &paths).unwrap();
        assert_eq!(
            fs::read(paths.plugins.join("capture/tool.exe")).unwrap(),
            b"new themed tool"
        );
        assert_eq!(fs::read(paths.settings()).unwrap(), before);
        assert_eq!(
            fs::read(data.join("settings.json")).unwrap(),
            b"user preferences"
        );
        assert_eq!(
            fs::read(data.join("history.json")).unwrap(),
            b"user history"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retires_only_legacy_uninstaller_and_preserves_user_data_and_preferences() {
        let (root, paths) = fixture("retired-uninstaller");
        let directory = paths.plugins.join("uninstaller");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("plugin.json"), serde_json::to_vec(&serde_json::json!({
            "schema_version":2, "id":"uninstaller", "name":"软件卸载", "version":"0.1.1",
            "runtime":"interactive", "executable":"ptools-uninstaller.exe", "actions":[{"id":"open", "title":"软件卸载"}]
        })).unwrap()).unwrap();
        fs::write(directory.join("ptools-uninstaller.exe"), b"legacy tool").unwrap();
        let data = paths.root.join("plugin-data/uninstaller");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("history.json"), b"retained legacy data").unwrap();
        let mut settings = Settings::default();
        settings.disabled_plugins.insert("uninstaller".into());
        write_json(&paths.settings(), &settings).unwrap();
        let before = fs::read(paths.settings()).unwrap();
        let mut cache = Cache::default();
        cache.plugins.insert(
            "uninstaller".into(),
            vec![Entry {
                id: "open".into(),
                title: "软件卸载".into(),
                subtitle: String::new(),
                keywords: vec![],
                target: Target::Plugin {
                    action: "open".into(),
                },
            }],
        );
        write_json(&paths.cache(), &cache).unwrap();
        // A failed cache update must leave the plugin identifiable for a retry.
        let cache_before = fs::read(paths.cache()).unwrap();
        let blocked_backup = paths.cache().with_extension("json.bak");
        fs::create_dir(&blocked_backup).unwrap();
        assert!(sync(&root, &paths).is_err());
        assert!(directory.exists());
        assert_eq!(fs::read(paths.cache()).unwrap(), cache_before);
        fs::remove_dir(blocked_backup).unwrap();
        // Retirement also works if the old executable was already removed.
        fs::remove_file(directory.join("ptools-uninstaller.exe")).unwrap();
        sync(&root, &paths).unwrap();
        assert!(!directory.exists());
        assert!(
            !read_json::<Cache>(&paths.cache())
                .unwrap()
                .plugins
                .contains_key("uninstaller")
        );
        assert_eq!(fs::read(paths.settings()).unwrap(), before);
        assert_eq!(
            fs::read(data.join("history.json")).unwrap(),
            b"retained legacy data"
        );
        // A user-supplied plugin with the same ID is not the retired native program.
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("plugin.json"), serde_json::to_vec(&serde_json::json!({
            "schema_version":1, "id":"uninstaller", "name":"自定义卸载入口", "version":"0.1.1",
            "runtime":"config", "entries":[{"id":"open", "title":"我的工具", "target":{"kind":"url", "url":"https://example.com"}}]
        })).unwrap()).unwrap();
        sync(&root, &paths).unwrap();
        assert!(directory.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn never_reinstalls_removed_plugin_or_downgrades_newer_plugin() {
        let (root, paths) = fixture("removed");
        bundle(&root, "0.1.0", b"old");
        sync(&root, &paths).unwrap();
        uninstall_plugin(&paths.plugins, "capture").unwrap();
        bundle(&root, "0.1.1", b"new");
        sync(&root, &paths).unwrap();
        assert!(!paths.plugins.join("capture").exists());
        bundle(&root, "0.2.0", b"newer installed");
        install_plugin(&root.join("plugins/capture"), &paths.plugins).unwrap();
        bundle(&root, "0.1.1", b"older bundle");
        sync(&root, &paths).unwrap();
        assert_eq!(
            fs::read(paths.plugins.join("capture/tool.exe")).unwrap(),
            b"newer installed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_upgrade_keeps_old_plugin_and_interrupted_swap_recovers() {
        let (root, paths) = fixture("recovery");
        bundle(&root, "0.1.0", b"old");
        sync(&root, &paths).unwrap();
        bundle(&root, "0.1.1", b"new");
        fs::remove_file(root.join("plugins/capture/tool.exe")).unwrap();
        assert!(sync(&root, &paths).is_err());
        assert_eq!(
            fs::read(paths.plugins.join("capture/tool.exe")).unwrap(),
            b"old"
        );
        bundle(&root, "0.1.0", b"old bundle");
        fs::rename(
            paths.plugins.join("capture"),
            paths.plugins.join(".bundled-backup-capture"),
        )
        .unwrap();
        sync(&root, &paths).unwrap();
        assert_eq!(
            fs::read(paths.plugins.join("capture/tool.exe")).unwrap(),
            b"old"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
