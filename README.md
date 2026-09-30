# ptools

Windows 原生应用启动器与轻量工具。Rust 本体 + 按需运行的 EXE / 配置插件，无 WebView、Node 或后台脚本引擎。

[下载 v0.1.0](https://github.com/jesongit/ptools/releases/tag/v0.1.0) · [插件开发](docs/plugin-development.md)

## 使用

解压 `ptools-0.1.0-windows-x64.zip`，双击 `ptools.exe`。第一次会安装随包的应用启动、截图与贴图、软件卸载插件，并在后台建立索引。已卸载的随包插件不会在下次启动时重新安装。

- `Alt + Space` 显示 / 隐藏搜索框。冲突时窗口会提示，输入“设置”可切换。
- 唤起时在鼠标所在屏幕的可用区域居中，结果和菜单展开后保持居中，适配不同屏幕缩放。
- 点击右上角 `···` 或按 `Tab` 打开内置菜单，与搜索框使用相同的列表样式；`Esc` 返回搜索。
- 右键托盘图标可勾选“自启动”开启开机启动，取消勾选即可关闭；点击“关闭”彻底退出程序，无需先打开搜索框。
- 输入应用名称、拼音或首字母，`↑↓` 选择，`Enter` 启动；鼠标双击也可启动。也可用 `Ctrl+J` 向下、`Ctrl+K` 向上选择，支持搜索结果、菜单和设置等列表；中文输入法正在组词时不接管这些按键。
- 列表左侧显示当前可见项目的序号，`Alt+数字` 只选中对应项（如 `Alt+1` 选中第一项），再按 `Enter` 才打开。滚动后按当前屏幕显示的序号重新对应；使用键盘上方数字键，无对应项目时不操作。
- `Esc` 从设置 / 插件管理返回搜索，再按一次隐藏。关闭窗口保留托盘；托盘菜单可以彻底退出。
- 输入“刷新”重新发现应用。索引刷新后插件进程退出。
- 输入“插件”安装、启用、停用或卸载插件。支持 `.ptplugin` / `.zip` 和插件目录里的 `plugin.json`。
- 输入“设置”修改快捷键、开机启动、查看数据和日志。开机启动默认关闭。
- 空查询显示最近使用；没有使用记录时只显示搜索提示。
- `Ctrl+1` 截图，`Ctrl+2` 贴出剪贴板，`Ctrl+3` 图片历史，`Alt+1` 延时 / 固定区域截图。搜索框显示时 `Alt+数字` 仍用于选中结果。
- 按住 `Win`，用鼠标左键拖动框选；先松开左键即可复制并在原选区贴图，再松开 `Win`。`Esc` 或先松开 `Win` 取消。停用截图插件会移除此手势。
- 输入“截图插件设置”配置缓存天数、圆角、取色格式和翻译服务；输入“软件卸载”管理桌面应用。功能和快捷键说明见 [原生工具](docs/native-tools.md)。

应用来源包括开始菜单快捷方式、桌面快捷方式和 Windows AppsFolder。不会扫描整个磁盘。新安装应用可通过“刷新”更新；每次程序启动也会后台刷新。某些没有注册入口的便携软件需要手动添加配置插件。

默认数据在 `%APPDATA%/ptools`。便携模式：在 `ptools.exe` 旁创建空文件 `portable.flag`，或者使用 `ptools.exe --portable`；数据位于旁边的 `data/`。已有用户数据不会自动搬迁。使用开机启动后请保持程序路径不变，移动后重新设置。

原生插件以当前用户身份执行，不是安全沙箱；只安装可信来源。第一版不支持 JS、WASM 或网页插件，也不兼容 uTools 插件。

## 构建

需要 Windows x64 和 Rust stable 工具链（GNU 或 MSVC，及对应链接器）。

```powershell
.\scripts\build.ps1
```

输出：`dist/ptools/ptools.exe`、便携分发 ZIP，以及 `applications`、`capture`、`uninstaller` 三个可单独安装的 `.ptplugin` 插件包。

程序、窗口和托盘图标使用 `assets/ptools.ico`。更换 `assets/ptools.png` 后运行 `./scripts/generate-icon.ps1` 重新生成多尺寸 ICO，再构建即可。

```powershell
.\dist\ptools\ptools.exe --help
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --refresh-index
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --search wx
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --doctor
```

CLI 操作已运行实例的数据时，请先退出该实例；GUI 内管理不需要退出。`--search`、`--plugins`、`--doctor` 为只读查询。开发者协议参见 `docs/plugin-development.md`（分发包中为 `plugin-development.md`）。

## 结构

- `crates/core`：清单、索引、搜索、安装、插件进程协议。
- `crates/desktop`：Win32 原生窗口、托盘、快捷键、设置和执行。
- `crates/applications`：独立应用发现插件，刷新后退出。
- `crates/tools`：按需启动的截图与卸载工具；离线 OCR 使用短期子进程。
- `plugins`：三个随包插件的清单。

Windows 10/11 x64 为目标平台；实际验收环境与资源占用记录在源码的 `docs/verification.md`（分发包中的 `verification.md`）。

## 验证

验证脚本需要 PowerShell 7，会创建隔离的数据目录和测试插件，不修改默认的用户设置。

```powershell
./scripts/verify.ps1
./scripts/check-package.ps1
./scripts/verify-tools.ps1
pwsh -STA -File ./scripts/verify-gesture.ps1
```

默认界面回归向测试窗口投递 `WM_HOTKEY`，同时单独检查系统热键注册与释放。可在普通桌面前台运行 `./scripts/verify.ps1 -RealKeyboard` 使用真实 `SendInput`；前台游戏、权限差异或输入钩子可能影响按键注入。普通快捷键使用系统 `RegisterHotKey`；Win 拖动截图由宿主的鼠标钩子识别，手势期间临时监听 Win / Esc。手势验收使用隔离窗口和数据目录，结束后恢复剪贴板、鼠标位置和前台窗口。
