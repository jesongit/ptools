# ptools

Windows 原生应用启动器与轻量工具。Rust 本体 + 按需运行的 EXE / 配置插件，无 WebView、Node 或常驻脚本引擎。

[下载 v0.1.0](https://github.com/jesongit/ptools/releases/tag/v0.1.0) · [插件开发](docs/plugin-development.md)

## 使用

解压 `ptools-0.1.9-windows-x64.zip`，双击 `ptools.exe` 后直接驻留托盘；按 `Alt + Space` 或点击托盘图标打开搜索框。需要启动时显示搜索框，可使用 `ptools.exe --show`。第一次会安装随包的应用启动、截图与贴图两个插件，并在后台建立索引。更新完整包时，先通过托盘“关闭”退出旧版，再启动新版；启动时会自动升级版本较旧的随包插件，保留配置、历史、使用记录和停用状态。本包随附截图与贴图插件 0.1.5，旧版插件会自动升级。已卸载的随包插件不会重新安装，也不会降级已安装的较新版本。0.1.3 移除了自研软件卸载器，升级会移除以前随包安装的旧卸载插件，保留其插件数据。

- `Alt + Space` 显示 / 隐藏搜索框。冲突时可点击托盘图标查看提示，输入“设置”可切换。
- 唤起时在鼠标所在屏幕的可用区域居中，结果和菜单展开后保持居中，适配不同屏幕缩放。
- 点击右上角 `···` 打开内置菜单；应用搜索模式也可按 `Tab` 打开。菜单与搜索框使用相同的列表样式，`Esc` 返回搜索。
- 右键托盘图标可勾选“自启动”开启开机启动，取消勾选即可关闭；点击“关闭”彻底退出程序，无需先打开搜索框。
- 输入应用名称、拼音或首字母，`↑↓` 选择，`Enter` 启动；鼠标双击也可启动。也可用 `Ctrl+J` 向下、`Ctrl+K` 向上选择，支持搜索结果、菜单和设置等列表；中文输入法正在组词时不接管这些按键。
- 应用搜索时输入 `=` 进入计算，左侧图标显示计算模式，输入框自动移除触发符号。随后输入 `2 + 3 * 4` 实时显示结果，输入框中按 `Enter` 复制；支持小数、科学计数法、括号、`+ - * / % ^` 和 `pi` / `e` / `π`。
- 应用搜索时输入 `>` 进入命令模式，左侧图标显示命令模式，输入框自动移除触发符号。随后输入 `Get-Date` 或 `ipconfig`，在输入框按 `Enter` 执行 PowerShell 命令。结果面板显示可选择、可滚动的输出、退出码与耗时；再次按 `Enter` 可重新执行，运行期间不会重复启动命令。命令中的 `>`、`=` 保持普通正文，不会切换模式。
- 计算和命令面板右侧“复制结果”或 `Ctrl+Shift+C` 复制结果正文；选中结果文字后可用 `Ctrl+C` 复制选区。`Tab` / `Shift+Tab` 在输入框、结果框和可用的复制按钮之间切换。工具模式按 `Esc` 返回进入前的搜索并恢复查询，再按 `Esc` 隐藏；正文已为空时按 `Backspace` / `Delete` 也可返回。`Ctrl+A` 后删除全文只清空正文，空输入上再次删除才退出工具。命令按次启动新进程，默认从用户目录开始；30 秒超时或输出超过 1 MiB 时停止，只保留已捕获输出。详见 [计算与命令输入](docs/quick-input.md)。
- 列表左侧显示当前可见项目的序号，`Alt+数字` 只选中对应项（如 `Alt+1` 选中第一项），再按 `Enter` 才打开。滚动后按当前屏幕显示的序号重新对应；使用键盘上方数字键，无对应项目时不操作。
- `Esc` 从设置 / 插件管理返回搜索，再按一次隐藏。关闭窗口保留托盘；托盘菜单可以彻底退出。
- 输入“刷新”重新发现应用。索引刷新后插件进程退出。
- 输入“设备管理器” / `sbglq` / `devmgmt.msc`，或“任务管理器”“显示设置”“网络连接”“下载文件夹”，直接打开 Windows 常用工具、设置和目录。系统入口归入应用启动插件，支持中文、拼音、首字母和英文命令别名；详见 [Windows 快开](docs/windows-launcher.md)。
- 输入“插件”安装、启用、停用或卸载插件。支持 `.ptplugin` / `.zip` 和插件目录里的 `plugin.json`。
- 输入“设置”修改快捷键、开机启动、查看数据和日志。开机启动默认关闭。
- 空查询显示最近使用；没有使用记录时只显示搜索提示。
- `Ctrl+1` 截图，`Ctrl+2` 贴出剪贴板，`Ctrl+3` 图片历史，`Alt+1` 延时 / 固定区域截图。搜索框显示时 `Alt+数字` 仍用于选中结果。
- 截图及贴图默认开启“文字框选”：直接拖选图中文字后按 `Ctrl+C` 复制，开关可与其他标注工具同时使用。移动模式下拖动无文字区域可移动截图选区或贴图，并清除之前的文字选区。
- 文字标注在点击的图片位置直接输入；`Enter` 确认，`Shift+Enter` 换行，`Esc` 取消。
- 标注工具合并为“矩形 / 椭圆”“直线 / 折线 / 箭头”“画笔 / 荧光笔”三组。点击小三角展开带图标与名称的选项，主按钮直接使用该窗口最近选择的工具；原快捷键仍可直接切换。
- 按住 `Win`，用鼠标左键拖动框选；先松开左键即可复制并在原选区贴图，再松开 `Win`。`Esc` 或先松开 `Win` 取消。停用截图插件会移除此手势。
- 输入“截图插件设置”配置缓存天数、圆角、取色格式和翻译服务。功能和快捷键说明见 [原生工具](docs/native-tools.md)。
- 软件卸载可直接使用 Geek 等现有工具；将它的快捷方式放到开始菜单或桌面后，刷新应用索引即可像普通应用一样搜索启动。也可搜索 Windows 的“已安装应用”或“程序和功能”。

应用来源包括开始菜单快捷方式、桌面快捷方式和 Windows AppsFolder，并补充内置 Windows 系统入口。不会扫描整个磁盘。新安装应用可通过“刷新”更新；每次程序启动也会后台刷新。某些没有注册入口的便携软件需要手动添加配置插件。

默认数据在 `%APPDATA%/ptools`。便携模式：在 `ptools.exe` 旁创建空文件 `portable.flag`，或者使用 `ptools.exe --portable`；数据位于旁边的 `data/`。已有用户数据不会自动搬迁。使用开机启动后请保持程序路径不变，移动后重新设置。

原生插件以当前用户身份执行，不是安全沙箱；只安装可信来源。第一版不支持 JS、WASM 或网页插件，也不兼容 uTools 插件。

## 构建

需要 Windows x64 和 Rust stable 工具链（GNU 或 MSVC，及对应链接器）。

```powershell
.\scripts\build.ps1
```

输出：`dist/ptools/ptools.exe`、便携分发 ZIP，以及 `applications`、`capture` 两个可单独安装的 `.ptplugin` 插件包。

程序、窗口和托盘图标使用 `assets/ptools.ico`。更换 `assets/ptools.png` 后运行 `./scripts/generate-icon.ps1` 重新生成多尺寸 ICO，再构建即可。

```powershell
.\dist\ptools\ptools.exe --help
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --refresh-index
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --search wx
.\dist\ptools\ptools.exe --data-dir D:\temp\ptools-test --doctor
```

CLI 操作已运行实例的数据时，请先退出该实例；GUI 内管理不需要退出。`--search`、`--plugins`、`--doctor` 为只读查询。开发者协议参见 `docs/plugin-development.md`（分发包中为 `plugin-development.md`）。

## 界面

采用 02 黑灰橙战术终端风格：原生搜索、计算与命令结果、插件与设置、截图贴图、图片历史和表单共用主题。计算使用大字号结果，命令输出使用可滚动的等宽文本区；二者共用标题、状态信息和复制操作。外部应用保留自己的界面。颜色、控件状态和各工具的应用规范见 [界面风格规范](docs/ui-style-guide.md)。

## 结构

- `crates/ui`：共享颜色令牌、原生控件主题和绘制辅助。
- `crates/core`：清单、索引、搜索、安装、插件进程协议。
- `crates/desktop`：Win32 原生窗口、托盘、快捷键、设置、实时计算和命令执行。
- `crates/applications`：独立应用发现与 Windows 快开插件，刷新后退出。
- `crates/tools`：按需启动的截图与贴图工具；离线 OCR 使用短期子进程。
- `plugins`：两个随包插件的清单。

Windows 10/11 x64 为目标平台；实际验收环境与资源占用记录在源码的 `docs/verification.md`（分发包中的 `verification.md`）。

## 验证

验证脚本需要 PowerShell 7，会创建隔离的数据目录和测试插件，不修改默认的用户设置。

```powershell
./scripts/verify.ps1
./scripts/check-package.ps1
./scripts/verify-tools.ps1
pwsh -STA -File ./scripts/verify-gesture.ps1
pwsh -STA -File ./scripts/verify-system-launcher.ps1
pwsh -STA -File ./scripts/verify-quick-input.ps1 -VerifyClipboard
pwsh -STA -File ./scripts/verify-capture-selection.ps1
pwsh -STA -File ./scripts/verify-capture-editing.ps1
pwsh -STA -File ./scripts/verify-capture-groups.ps1
pwsh -STA -File ./scripts/verify-long-capture.ps1
```

默认界面回归向测试窗口投递 `WM_HOTKEY`，同时单独检查系统热键注册与释放。可在普通桌面前台运行 `./scripts/verify.ps1 -RealKeyboard` 使用真实 `SendInput`；前台游戏、权限差异或输入钩子可能影响按键注入。普通快捷键使用系统 `RegisterHotKey`；Win 拖动截图由宿主的鼠标钩子识别，手势期间临时监听 Win / Esc。手势验收使用隔离窗口和数据目录，结束后恢复剪贴板、鼠标位置和前台窗口。
