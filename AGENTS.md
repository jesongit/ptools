# 本机运行约定

- 用户要求日常运行及最终交付统一使用 `D:\code\ptools\run\ptools.exe`，数据目录固定为 `D:\code\ptools\run\data`。
- `dist/` 只存构建和发布成品；完成后通过 `scripts/install-run.ps1` 部署到 `run/`，并核对运行进程、数据目录和已启用的开机启动项。
- 保留 `run/portable.flag`，使直接双击程序也使用 `run/data`。`run/` 不提交到 Git。
- 隔离验证使用 `artifacts/` 下的独立数据，完成后关闭测试实例，最终只保留 `run/` 中的日常实例。
