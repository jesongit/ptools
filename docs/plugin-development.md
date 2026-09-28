# ptools 插件协议 v1

插件是一个目录，根部包含 `plugin.json`。打包时将目录内容压缩为 ZIP，可使用 `.ptplugin` 扩展名。安装后无需重新编译本体。插件目录名称由清单 ID 决定。

## 配置插件

```json
{
  "schema_version": 1,
  "id": "my-shortcuts",
  "name": "我的快捷入口",
  "version": "0.1.0",
  "runtime": "config",
  "entries": [
    {
      "id": "example",
      "title": "示例网站",
      "keywords": ["example"],
      "target": { "kind": "url", "url": "https://example.com" }
    }
  ]
}
```

`target` 支持 `path`（绝对路径）、`url`（http/https）、`app`（Windows AppsFolder 标识）：

```json
{"kind":"path","path":"C:\\Windows\\System32\\notepad.exe"}
```

本体执行的是独立的启动目标，不拼接 shell 命令。v1 不支持自定义命令参数和实时查询回调。

## EXE 插件

```json
{
  "schema_version": 1,
  "id": "applications",
  "name": "应用启动",
  "version": "0.1.0",
  "runtime": "executable",
  "executable": "ptools-applications.exe"
}
```

入口是插件目录内的 `.exe`，可用 Rust、C/C++ 或其他能交付 Windows 程序的语言实现。不得依赖主程序的当前工作目录；执行时工作目录设为插件目录。

本体写入 UTF-8 JSON 到 stdin，追加换行并关闭 stdin：

```json
{"protocol_version":1,"operation":"index"}
```

插件返回一个 UTF-8 JSON 对象到 stdout，随后退出，状态码必须为 0：

```json
{
  "protocol_version": 1,
  "entries": [
    {
      "id": "notepad",
      "title": "记事本",
      "subtitle": "Windows 应用",
      "keywords": ["notepad"],
      "target": {"kind":"path","path":"C:\\Windows\\System32\\notepad.exe"}
    }
  ],
  "warnings": []
}
```

条目 ID 在插件内唯一，跨刷新尽量稳定。stdout 不能输出日志；需要向用户报告的部分失败请写入 `warnings`。

## 生命周期与边界

本体每次启动、手动刷新、安装或启用插件时索引。EXE 超时 30 秒将被结束；输出限制 8 MB，每个插件最多 20000 条。失败保留此插件之前的索引并记录警告。插件退出后不需要常驻，日常搜索由本体完成。v1 插件不得创建常驻子进程，宿主通过 Windows Job Object 管理插件进程树，在任务结束或宿主退出时回收。

禁用插件后不执行、不展示入口。卸载会清除插件目录、索引与使用记录。重新安装需要先卸载；第一版不做原位更新和市场。

安装包解压大小上限 128 MB，最多 4096 项，拒绝路径穿越、符号链接。原生插件拥有当前用户的系统权限，清单不构成权限沙箱。WebView、脚本运行时和 WASM 属于未来可添加的运行类型，尚未实现。
