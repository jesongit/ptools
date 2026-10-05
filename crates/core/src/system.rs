//! Built-in Windows launch destinations. Targets are selected by ID from this
//! catalog, so neither cached entries nor plugins can supply an arbitrary URI.

use crate::{Entry, Result, Target};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemLocation {
    File(&'static str),
    Uri(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct SystemCommand {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub keywords: &'static [&'static str],
    pub location: SystemLocation,
}

macro_rules! file {
    ($id:literal, $title:literal, $category:literal, $path:literal, [$($keyword:literal),* $(,)?]) => {
        SystemCommand {
            id: $id,
            title: $title,
            category: $category,
            keywords: &[$($keyword),*],
            location: SystemLocation::File($path),
        }
    };
}

macro_rules! uri {
    ($id:literal, $title:literal, $category:literal, $uri:literal, [$($keyword:literal),* $(,)?]) => {
        SystemCommand {
            id: $id,
            title: $title,
            category: $category,
            keywords: &[$($keyword),*],
            location: SystemLocation::Uri($uri),
        }
    };
}

// Settings URI reference:
// https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings
// Shell destinations use known-folder names, letting Explorer honor redirected
// user folders instead of assuming their paths underneath USERPROFILE.
pub const SYSTEM_COMMANDS: &[SystemCommand] = &[
    file!(
        "device-manager",
        "设备管理器",
        "系统工具",
        "devmgmt.msc",
        [
            "device manager",
            "devmgmt",
            "devmgmt.msc",
            "设备",
            "硬件",
            "驱动"
        ]
    ),
    file!(
        "task-manager",
        "任务管理器",
        "系统工具",
        "Taskmgr.exe",
        [
            "task manager",
            "taskmgr",
            "taskmgr.exe",
            "任务",
            "进程",
            "性能"
        ]
    ),
    file!(
        "disk-management",
        "磁盘管理",
        "系统工具",
        "diskmgmt.msc",
        [
            "disk management",
            "diskmgmt",
            "diskmgmt.msc",
            "硬盘",
            "磁盘",
            "分区"
        ]
    ),
    file!(
        "services",
        "服务管理",
        "系统工具",
        "services.msc",
        ["services", "services.msc", "服务"]
    ),
    file!(
        "event-viewer",
        "事件查看器",
        "系统工具",
        "eventvwr.msc",
        ["event viewer", "eventvwr", "eventvwr.msc", "事件", "日志"]
    ),
    file!(
        "computer-management",
        "计算机管理",
        "系统工具",
        "compmgmt.msc",
        ["computer management", "compmgmt", "compmgmt.msc", "计算机"]
    ),
    file!(
        "control-panel",
        "控制面板",
        "系统工具",
        "control.exe",
        ["control panel", "control", "control.exe"]
    ),
    // regedt32 is the System32 compatibility launcher for Windows\regedit.exe.
    file!(
        "registry-editor",
        "注册表编辑器",
        "系统工具",
        "regedt32.exe",
        [
            "registry editor",
            "regedit",
            "regedit.exe",
            "regedt32",
            "regedt32.exe",
            "注册表"
        ]
    ),
    file!(
        "resource-monitor",
        "资源监视器",
        "系统工具",
        "resmon.exe",
        ["resource monitor", "resmon", "resmon.exe", "资源", "性能"]
    ),
    file!(
        "system-information",
        "系统信息",
        "系统工具",
        "msinfo32.exe",
        [
            "system information",
            "msinfo32",
            "msinfo32.exe",
            "系统配置",
            "硬件信息"
        ]
    ),
    file!(
        "advanced-system-settings",
        "高级系统设置",
        "系统工具",
        "SystemPropertiesAdvanced.exe",
        [
            "advanced system settings",
            "SystemPropertiesAdvanced",
            "SystemPropertiesAdvanced.exe",
            "环境变量",
            "environment variables",
            "path",
            "系统属性"
        ]
    ),
    file!(
        "system-configuration",
        "系统配置",
        "系统工具",
        "msconfig.exe",
        [
            "system configuration",
            "msconfig",
            "msconfig.exe",
            "引导",
            "启动配置"
        ]
    ),
    file!(
        "group-policy",
        "本地组策略编辑器",
        "系统工具",
        "gpedit.msc",
        ["group policy", "gpedit", "gpedit.msc", "组策略"]
    ),
    file!(
        "network-connections",
        "网络连接",
        "控制面板",
        "ncpa.cpl",
        [
            "network connections",
            "ncpa",
            "ncpa.cpl",
            "网卡",
            "适配器",
            "网络"
        ]
    ),
    file!(
        "sound-control-panel",
        "声音控制面板",
        "控制面板",
        "mmsys.cpl",
        [
            "sound control panel",
            "mmsys",
            "mmsys.cpl",
            "播放设备",
            "录音设备",
            "扬声器",
            "麦克风"
        ]
    ),
    file!(
        "programs-features",
        "程序和功能",
        "控制面板",
        "appwiz.cpl",
        [
            "programs and features",
            "appwiz",
            "appwiz.cpl",
            "卸载",
            "软件",
            "程序"
        ]
    ),
    file!(
        "power-options",
        "电源选项",
        "控制面板",
        "powercfg.cpl",
        [
            "power options",
            "powercfg",
            "powercfg.cpl",
            "电源计划",
            "休眠"
        ]
    ),
    file!(
        "command-prompt",
        "命令提示符",
        "终端",
        "cmd.exe",
        ["command prompt", "cmd", "cmd.exe", "终端", "命令行"]
    ),
    file!(
        "powershell",
        "Windows PowerShell",
        "终端",
        "WindowsPowerShell\\v1.0\\powershell.exe",
        ["powershell", "powershell.exe", "终端", "命令行"]
    ),
    file!(
        "task-scheduler",
        "任务计划程序",
        "系统工具",
        "taskschd.msc",
        [
            "task scheduler",
            "taskschd",
            "taskschd.msc",
            "计划任务",
            "定时任务"
        ]
    ),
    file!(
        "disk-cleanup",
        "磁盘清理",
        "系统工具",
        "cleanmgr.exe",
        [
            "disk cleanup",
            "cleanmgr",
            "cleanmgr.exe",
            "磁盘",
            "清理",
            "临时文件"
        ]
    ),
    file!(
        "firewall",
        "高级安全 Windows 防火墙",
        "系统工具",
        "wf.msc",
        [
            "windows firewall",
            "firewall",
            "wf",
            "wf.msc",
            "防火墙",
            "网络安全"
        ]
    ),
    file!(
        "remote-desktop-client",
        "远程桌面连接",
        "系统工具",
        "mstsc.exe",
        [
            "remote desktop connection",
            "mstsc",
            "mstsc.exe",
            "rdp",
            "远程连接"
        ]
    ),
    uri!(
        "settings",
        "Windows 设置",
        "系统设置",
        "ms-settings:",
        ["settings", "ms-settings:", "设置"]
    ),
    uri!(
        "display-settings",
        "显示设置",
        "系统设置",
        "ms-settings:display",
        [
            "display",
            "ms-settings:display",
            "显示器",
            "屏幕",
            "分辨率",
            "缩放"
        ]
    ),
    uri!(
        "sound-settings",
        "声音设置",
        "系统设置",
        "ms-settings:sound",
        [
            "sound",
            "audio",
            "ms-settings:sound",
            "声音",
            "音量",
            "扬声器",
            "麦克风"
        ]
    ),
    uri!(
        "volume-mixer",
        "音量混合器",
        "系统设置",
        "ms-settings:apps-volume",
        [
            "volume mixer",
            "mixer",
            "ms-settings:apps-volume",
            "音量",
            "混音器",
            "应用音量"
        ]
    ),
    uri!(
        "bluetooth-settings",
        "蓝牙设置",
        "系统设置",
        "ms-settings:bluetooth",
        ["bluetooth", "ms-settings:bluetooth", "蓝牙", "设备配对"]
    ),
    uri!(
        "printer-settings",
        "打印机和扫描仪",
        "系统设置",
        "ms-settings:printers",
        [
            "printers",
            "scanners",
            "ms-settings:printers",
            "打印机",
            "扫描仪"
        ]
    ),
    uri!(
        "mouse-settings",
        "鼠标设置",
        "系统设置",
        "ms-settings:mousetouchpad",
        [
            "mouse",
            "touchpad",
            "ms-settings:mousetouchpad",
            "鼠标",
            "触摸板"
        ]
    ),
    uri!(
        "network-settings",
        "网络和 Internet",
        "系统设置",
        "ms-settings:network-status",
        [
            "network",
            "internet",
            "ms-settings:network-status",
            "网络",
            "联网"
        ]
    ),
    uri!(
        "wifi-settings",
        "Wi-Fi 设置",
        "系统设置",
        "ms-settings:network-wifi",
        [
            "wifi",
            "wi-fi",
            "ms-settings:network-wifi",
            "无线",
            "无线网络"
        ]
    ),
    uri!(
        "proxy-settings",
        "代理设置",
        "系统设置",
        "ms-settings:network-proxy",
        ["proxy", "ms-settings:network-proxy", "代理", "网络代理"]
    ),
    uri!(
        "vpn-settings",
        "VPN 设置",
        "系统设置",
        "ms-settings:network-vpn",
        ["vpn", "ms-settings:network-vpn", "虚拟专用网络"]
    ),
    uri!(
        "apps-settings",
        "已安装的应用",
        "系统设置",
        "ms-settings:appsfeatures",
        [
            "apps",
            "apps features",
            "ms-settings:appsfeatures",
            "应用",
            "软件",
            "卸载"
        ]
    ),
    uri!(
        "default-apps-settings",
        "默认应用",
        "系统设置",
        "ms-settings:defaultapps",
        [
            "default apps",
            "ms-settings:defaultapps",
            "默认应用",
            "默认浏览器",
            "文件关联"
        ]
    ),
    uri!(
        "startup-apps-settings",
        "启动应用",
        "系统设置",
        "ms-settings:startupapps",
        [
            "startup apps",
            "ms-settings:startupapps",
            "开机启动",
            "自启动"
        ]
    ),
    uri!(
        "power-settings",
        "电源和睡眠",
        "系统设置",
        "ms-settings:powersleep",
        [
            "power",
            "sleep",
            "ms-settings:powersleep",
            "电源",
            "睡眠",
            "电池"
        ]
    ),
    uri!(
        "storage-settings",
        "存储设置",
        "系统设置",
        "ms-settings:storagesense",
        ["storage", "ms-settings:storagesense", "存储", "磁盘空间"]
    ),
    uri!(
        "clipboard-settings",
        "剪贴板设置",
        "系统设置",
        "ms-settings:clipboard",
        ["clipboard", "ms-settings:clipboard", "剪贴板", "剪贴板历史"]
    ),
    uri!(
        "notifications-settings",
        "通知设置",
        "系统设置",
        "ms-settings:notifications",
        ["notifications", "ms-settings:notifications", "通知"]
    ),
    uri!(
        "remote-desktop-settings",
        "远程桌面设置",
        "系统设置",
        "ms-settings:remotedesktop",
        [
            "remote desktop",
            "ms-settings:remotedesktop",
            "rdp",
            "远程桌面"
        ]
    ),
    uri!(
        "date-time-settings",
        "日期和时间",
        "系统设置",
        "ms-settings:dateandtime",
        [
            "date and time",
            "ms-settings:dateandtime",
            "日期",
            "时间",
            "时区"
        ]
    ),
    uri!(
        "language-settings",
        "语言设置",
        "系统设置",
        "ms-settings:regionlanguage",
        [
            "language",
            "region",
            "ms-settings:regionlanguage",
            "语言",
            "输入法",
            "键盘",
            "地区"
        ]
    ),
    uri!(
        "windows-update",
        "Windows 更新",
        "系统设置",
        "ms-settings:windowsupdate",
        [
            "windows update",
            "ms-settings:windowsupdate",
            "更新",
            "系统更新"
        ]
    ),
    uri!(
        "windows-security",
        "Windows 安全中心",
        "系统设置",
        "ms-settings:windowsdefender",
        [
            "windows security",
            "defender",
            "ms-settings:windowsdefender",
            "安全中心",
            "病毒防护"
        ]
    ),
    uri!(
        "recovery-settings",
        "恢复设置",
        "系统设置",
        "ms-settings:recovery",
        ["recovery", "ms-settings:recovery", "恢复", "重置电脑"]
    ),
    uri!(
        "about-settings",
        "关于此电脑",
        "系统设置",
        "ms-settings:about",
        ["about", "ms-settings:about", "系统版本", "电脑信息"]
    ),
    uri!(
        "personalization-settings",
        "个性化设置",
        "系统设置",
        "ms-settings:personalization",
        [
            "personalization",
            "ms-settings:personalization",
            "个性化",
            "壁纸",
            "主题"
        ]
    ),
    uri!(
        "taskbar-settings",
        "任务栏设置",
        "系统设置",
        "ms-settings:taskbar",
        ["taskbar", "ms-settings:taskbar", "任务栏"]
    ),
    uri!(
        "account-settings",
        "账户信息",
        "系统设置",
        "ms-settings:yourinfo",
        [
            "account",
            "your info",
            "ms-settings:yourinfo",
            "账户",
            "用户"
        ]
    ),
    uri!(
        "this-pc",
        "此电脑",
        "系统文件夹",
        "shell:MyComputerFolder",
        [
            "this pc",
            "my computer",
            "shell:MyComputerFolder",
            "我的电脑",
            "计算机",
            "硬盘"
        ]
    ),
    uri!(
        "recycle-bin",
        "回收站",
        "系统文件夹",
        "shell:RecycleBinFolder",
        ["recycle bin", "shell:RecycleBinFolder", "回收站"]
    ),
    uri!(
        "downloads",
        "下载文件夹",
        "系统文件夹",
        "shell:Downloads",
        ["downloads", "shell:Downloads", "下载"]
    ),
    uri!(
        "desktop",
        "桌面文件夹",
        "系统文件夹",
        "shell:Desktop",
        ["desktop", "shell:Desktop", "桌面"]
    ),
    uri!(
        "documents",
        "文档文件夹",
        "系统文件夹",
        "shell:Personal",
        ["documents", "shell:Personal", "文档", "我的文档"]
    ),
    uri!(
        "pictures",
        "图片文件夹",
        "系统文件夹",
        "shell:My Pictures",
        ["pictures", "shell:My Pictures", "图片", "我的图片"]
    ),
    uri!(
        "startup-folder",
        "启动文件夹",
        "系统文件夹",
        "shell:Startup",
        [
            "startup folder",
            "shell:Startup",
            "启动目录",
            "开机启动",
            "自启动"
        ]
    ),
];

pub fn system_command(id: &str) -> Option<&'static SystemCommand> {
    SYSTEM_COMMANDS.iter().find(|command| command.id == id)
}

/// Resolve the operating system directory without trusting the PATH or the
/// process's environment variables.
#[cfg(windows)]
pub fn system_directory() -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: the buffer contains `buffer.len()` writable UTF-16 elements.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(format!(
                "无法获取 Windows 系统目录：{}",
                std::io::Error::last_os_error()
            ));
        }
        if (length as usize) < buffer.len() {
            let path = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
            if !path.is_absolute() {
                return Err("Windows 系统目录不是绝对路径".into());
            }
            return Ok(path);
        }
        let capacity = length as usize + 1;
        if capacity > 32768 {
            return Err("Windows 系统目录过长".into());
        }
        buffer.resize(capacity, 0);
    }
}

#[cfg(not(windows))]
pub fn system_directory() -> Result<PathBuf> {
    Err("系统启动入口仅支持 Windows".into())
}

impl SystemCommand {
    pub fn resolve(&self) -> Result<String> {
        // Resolve the registered destination rather than caller-provided fields.
        let command = system_command(self.id).ok_or("未知的 Windows 系统入口")?;
        match command.location {
            SystemLocation::File(relative) => Ok(system_directory()?
                .join(relative)
                .to_string_lossy()
                .into_owned()),
            SystemLocation::Uri(uri) if cfg!(windows) => Ok(uri.into()),
            SystemLocation::Uri(_) => Err("系统启动入口仅支持 Windows".into()),
        }
    }

    pub fn available(&self) -> bool {
        let Some(command) = system_command(self.id) else {
            return false;
        };
        match command.location {
            SystemLocation::File(_) => command
                .resolve()
                .is_ok_and(|path| PathBuf::from(path).is_file()),
            // URI pages are opened by Windows. Their specific availability can
            // vary with Windows version, edition, and installed hardware.
            SystemLocation::Uri(_) => cfg!(windows),
        }
    }

    pub fn entry(&self) -> Entry {
        let mut keywords = vec!["windows".into(), "系统".into(), self.category.into()];
        keywords.extend(self.keywords.iter().map(|keyword| (*keyword).into()));
        Entry {
            id: format!("system:{}", self.id),
            title: self.title.into(),
            subtitle: format!("Windows · {}", self.category),
            keywords,
            target: Target::System { id: self.id.into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cache, Settings, build_index, search, validate_entry};
    use std::collections::BTreeSet;

    #[test]
    fn catalog_has_unique_ids_and_valid_entries() {
        let mut ids = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        for command in SYSTEM_COMMANDS {
            assert!(ids.insert(command.id), "duplicate ID: {}", command.id);
            assert!(!command.category.is_empty());
            assert!(!command.keywords.is_empty());
            let location = match command.location {
                SystemLocation::File(path) => {
                    assert!(!std::path::Path::new(path).is_absolute());
                    assert!(!path.contains(".."));
                    path
                }
                SystemLocation::Uri(uri) => {
                    assert!(uri.starts_with("ms-settings:") || uri.starts_with("shell:"));
                    uri
                }
            };
            assert!(
                destinations.insert(location),
                "duplicate destination: {location}"
            );
            let entry = command.entry();
            assert_eq!(entry.id, format!("system:{}", command.id));
            validate_entry(&entry).unwrap();
            assert!(matches!(entry.target, Target::System { id } if id == command.id));
        }
    }

    #[test]
    fn system_entries_support_chinese_pinyin_initials_english_and_commands() {
        let mut cache = Cache::default();
        cache.plugins.insert(
            "apps".into(),
            SYSTEM_COMMANDS.iter().map(SystemCommand::entry).collect(),
        );
        let settings = Settings::default();
        let index = build_index(&cache, &settings);
        for query in [
            "设备管理器",
            "shebeiguanliqi",
            "sbglq",
            "device manager",
            "devmgmt.msc",
        ] {
            let matches = search(&index, query, &settings, 10);
            assert!(!matches.is_empty(), "no match for {query}");
            assert_eq!(
                index[matches[0]].entry.id, "system:device-manager",
                "query: {query}"
            );
        }
        for (query, id) in [
            ("环境变量", "advanced-system-settings"),
            ("downloads", "downloads"),
            ("ms-settings:display", "display-settings"),
        ] {
            let matches = search(&index, query, &settings, 10);
            assert!(!matches.is_empty(), "no match for {query}");
            assert_eq!(index[matches[0]].entry.id, format!("system:{id}"));
        }
        let matches = search(&index, "系统设置", &settings, index.len());
        for id in ["display-settings", "volume-mixer"] {
            assert!(
                matches
                    .iter()
                    .any(|index_id| index[*index_id].entry.id == format!("system:{id}"))
            );
        }
    }

    #[test]
    fn unknown_ids_and_unregistered_uris_are_rejected() {
        for id in [
            "",
            "device-manager ",
            "../cmd.exe",
            "ms-settings:display",
            "shell:Downloads",
            "shutdown",
        ] {
            assert!(system_command(id).is_none(), "unexpected ID: {id}");
            let entry = Entry {
                id: "invalid".into(),
                title: "Invalid system target".into(),
                subtitle: String::new(),
                keywords: vec![],
                target: Target::System { id: id.into() },
            };
            assert!(validate_entry(&entry).is_err());
        }
        let injected = SystemCommand {
            id: "unregistered",
            title: "Injected",
            category: "test",
            keywords: &[],
            location: SystemLocation::Uri("shell:unregistered"),
        };
        assert!(injected.resolve().is_err());
        assert!(!injected.available());
    }

    #[cfg(windows)]
    #[test]
    fn file_targets_resolve_under_absolute_system_directory() {
        let directory = system_directory().unwrap();
        assert!(directory.is_absolute());
        for command in SYSTEM_COMMANDS {
            if let SystemLocation::File(relative) = command.location {
                let path = PathBuf::from(command.resolve().unwrap());
                assert!(path.is_absolute(), "{}", command.id);
                assert_eq!(path, directory.join(relative));
                assert_eq!(command.available(), path.is_file());
            }
        }
        assert!(system_command("device-manager").unwrap().available());
        assert!(system_command("registry-editor").unwrap().available());
    }

    #[cfg(windows)]
    #[test]
    fn registered_id_always_resolves_its_fixed_destination() {
        let injected = SystemCommand {
            id: "display-settings",
            title: "Injected",
            category: "test",
            keywords: &[],
            location: SystemLocation::Uri("shell:unregistered"),
        };
        assert_eq!(injected.resolve().unwrap(), "ms-settings:display");
    }
}
