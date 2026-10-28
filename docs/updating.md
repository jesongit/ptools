# 更新与发布

## 日常使用

本机固定运行目录为 `D:\code\ptools\run`，数据目录为 `run\data`。保留 `run\portable.flag`，直接双击 `run\ptools.exe` 也会使用这里的数据。`run/` 已加入 Git 忽略列表。

本地构建完成后，用 `pwsh -File scripts/install-run.ps1 -PackageRoot dist/release-x.y.z/ptools -DataSource run/data` 将完整包部署到固定目录并启动。首次从旧用户目录迁移时省略 `-DataSource`；脚本会复制现有数据并逐文件验证 SHA256，保留原目录，改写已开启的开机启动项。

启动时以及运行期间每 24 小时后台检查正式版本。默认先访问内网 Forgejo；网络、授权、Release 格式或附件不完整时回退 GitHub。只接受更高版本，忽略草稿和预发布，不降级。内网可用但没有更高版本时，以内网结果为准。

搜索“更新”，或打开菜单、设置、托盘菜单，选择“检查更新”。有新版时入口变成“更新到 x.y.z”；确认一次后下载、验证 SHA256 和程序版本，再关闭截图、贴图和主程序，原地替换完整包并重新启动。设置里可以关闭后台自动检查，手动检查仍可使用。失败时保留当前程序；替换失败会恢复已备份的文件。错误写入数据目录 `last-update.txt`。

更新只替换主程序、随包插件和随包文档，保留 `data/`、`portable.flag`、其他用户文件和现有开机启动路径。新版启动时按既有规则升级随包插件。程序目录需要当前用户的写入权限。中断或失败的更新工作目录 `.ptools-update-*` 保留在程序目录中，里面的 `old/` 是替换前备份；确认更新完成后，可在退出 ptools 时手动清理这些目录。

目前平台为 Windows x64，附件必须包含：

- `ptools-x.y.z-windows-x64.zip`：`scripts/build.ps1` 生成的完整包，包含 `ptools.exe` 和两个随包插件。
- `SHA256SUMS.txt`：包含上述 ZIP 的 SHA256。

网页自动生成的 Source code ZIP/TAR 是源码，不能用于更新。

## 内网认证

默认 API：`http://192.168.31.10:13000/api/v1/repos/posase/ptools/releases/latest`。

Git 仓库的 SSH 密钥只授权 Git 操作。Release 查询、附件下载和上传走 HTTP API；需要认证时，更新客户端优先读取 `PTOOLS_UPDATE_TOKEN` 环境变量，否则读取当前用户的 `%USERPROFILE%\.ssh\.env`。可用 `PTOOLS_UPDATE_TOKEN_FILE` 指定其他凭据文件。支持单行令牌、`FORGEJO_TOKEN=值` 和 `FORGEJO-TOKEN:值` 等格式；文件只作为数据解析，不执行其中的命令，也不导入其他变量。令牌只发送给所选内网 API 的同源地址，不会发送给 GitHub，也不复制到设置、日志或发布包。发布脚本使用同一凭据文件，也支持单独配置 `FORGEJO_TOKEN` 写入令牌。

高级配置在数据目录 `settings.json`：`update_lan_api`、`update_github_api` 可修改地址；`auto_update_check` 控制自动检查。默认 GitHub API 为 `https://api.github.com/repos/jesongit/ptools/releases/latest`。暂时无法授权内网时，客户端会自动尝试 GitHub，并在更新入口显示回退原因。

命令行可使用 `ptools.exe --check-update` 查看检查结果；退出 GUI 后使用 `ptools.exe --update` 下载、更新并重新启动。便携标记或 `--data-dir` 同样适用。

## 发布完整包

Git 推送只同步提交和标签，Release 说明与附件属于平台数据，必须另外创建或迁移。因此只把仓库推到内网，并不会出现 GitHub 上已上传的软件 ZIP。

1. 更新工作区版本和说明，运行 `pwsh -File scripts/build.ps1 -OutputRoot dist/release-x.y.z`。
2. 运行 `pwsh -File scripts/check-package.ps1 -DistributionRoot dist/release-x.y.z`，验证完整包。
3. 提交改动，在该提交创建 `vx.y.z`，把提交和标签推送到需要发布的远端。
4. 查看上传计划：`pwsh -File scripts/publish-release.ps1 -Source both -DistributionRoot dist/release-x.y.z -WhatIf`。
5. 上传草稿：去掉 `-WhatIf`。检查无误后，再增加 `-Publish` 发布。也可以首次执行时直接加 `-Publish`。

`-Source lan` 只发布内网，`github` 只发布 GitHub，`both` 发布两边。脚本上传校验文件里列出的完整包和插件包，以及校验文件本身；所有附件到齐后才把新草稿设为正式 Release。重复执行会验证已有附件的哈希，不静默覆盖不同内容。GitHub 使用 `GH_TOKEN`、`GITHUB_TOKEN` 或已登录的 `gh`，Forgejo 使用 `FORGEJO_TOKEN` 或 `PTOOLS_UPDATE_TOKEN`。不要将令牌写入脚本或命令行参数。

两个发布源需要分别上传相同的成品附件。推荐直接复用同一份构建输出，无需为 Forgejo 重新设计软件包格式。
