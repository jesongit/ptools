//! Full-package updates. The installed directory stays stable, so shortcuts and
//! autostart keep working. Only release-owned files are replaced; data is separate.
use ptools_core::{Paths, Result, Settings, load_plugin, now};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const MAX_PACKAGE: u64 = 128 * 1024 * 1024;
const MAX_EXPANDED: u64 = 512 * 1024 * 1024;
const REQUIRED: &[&str] = &[
    "ptools.exe",
    "plugins/applications/plugin.json",
    "plugins/applications/ptools-applications.exe",
    "plugins/capture/plugin.json",
    "plugins/capture/ptools-capture.exe",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub version: String,
    pub source: String,
    pub api: String,
    pub package: String,
    pub checksum: String,
    pub filename: String,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub update: Option<Candidate>,
    pub message: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn version(text: &str) -> Result<[u64; 3]> {
    let value = text.strip_prefix('v').unwrap_or(text);
    let numbers = value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
                return Err("版本必须为正式的 x.y.z".to_string());
            }
            part.parse::<u64>().map_err(|_| "版本号过大".to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    numbers
        .try_into()
        .map_err(|_| "版本必须为正式的 x.y.z".into())
}

// Parse origins with the same URI parser as the HTTP client. Credentials are
// never attached to a different origin, including an asset on another host.
fn origin(url: &str) -> Result<String> {
    let uri: ureq::http::Uri = url.parse().map_err(|_| "更新地址无效")?;
    let scheme = uri.scheme_str().ok_or("更新地址缺少协议")?;
    if !matches!(scheme, "http" | "https") {
        return Err("更新地址仅支持 HTTP / HTTPS".into());
    }
    let authority = uri.authority().ok_or("更新地址缺少主机")?.as_str();
    if authority.contains('@') {
        return Err("更新地址不能包含账号或密码".into());
    }
    Ok(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

fn parse_token(text: &str) -> Option<String> {
    let lines: Vec<_> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let clean = |value: &str| -> Option<String> {
        let value = value.trim().trim_matches(['\'', '"']);
        (!value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic())).then(|| value.to_owned())
    };
    for line in &lines {
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if let Some((key, value)) = line.split_once(['=', ':', '：']) {
            if matches!(
                key.trim().to_ascii_uppercase().replace('-', "_").as_str(),
                "PTOOLS_UPDATE_TOKEN" | "FORGEJO_TOKEN" | "GITEA_TOKEN" | "TOKEN" | "令牌"
            ) {
                return clean(value);
            }
        }
    }
    if lines.len() == 1 && !lines[0].contains(['=', ':', '：']) {
        clean(lines[0])
    } else {
        None
    }
}

fn update_token() -> Option<String> {
    // Unit-test HTTP fixtures must never read or transmit the user's real token.
    if cfg!(test) {
        return None;
    }
    if let Ok(value) = std::env::var("PTOOLS_UPDATE_TOKEN")
        && !value.trim().is_empty()
    {
        return Some(value.trim().to_owned());
    }
    let file = std::env::var_os("PTOOLS_UPDATE_TOKEN_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".ssh/.env"))
        })?;
    // Read credential data only; never execute or import a dotenv file as code.
    if fs::metadata(&file).ok()?.len() > 65536 {
        return None;
    }
    parse_token(&fs::read_to_string(file).ok()?)
}

fn fetch(url: &str, api: &str, source: &str, limit: u64, timeout: u64) -> Result<Vec<u8>> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let destination = origin(url)?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        );
    // LAN traffic should remain direct even when the parent shell has a proxy.
    let config = if source == "内网" {
        config.proxy(None)
    } else {
        config
    };
    let agent = config.build().new_agent();
    let mut request = agent
        .get(url)
        .header("User-Agent", "ptools-updater")
        .header("Accept", "application/octet-stream, application/json");
    if source == "内网" && destination == origin(api)? {
        if let Some(token) = update_token() {
            request = request.header("Authorization", format!("token {token}"));
        }
    }
    // ureq's default redirect policy strips Authorization on cross-origin hops.
    let mut response = request.call().map_err(|e| match e {
        ureq::Error::StatusCode(401 | 403) if source == "内网" => {
            "更新源需要授权，请配置 PTOOLS_UPDATE_TOKEN".into()
        }
        ureq::Error::StatusCode(401 | 403) => "GitHub 访问受限，请稍后重试".into(),
        ureq::Error::StatusCode(404) => "更新源尚未发布正式 Release".into(),
        ureq::Error::StatusCode(code) => format!("更新源返回 HTTP {code}"),
        _ => "无法连接更新源，请检查网络后重试".into(),
    })?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "更新下载中断，请重试")?;
    if bytes.len() as u64 > limit {
        return Err("更新文件超过大小限制".into());
    }
    Ok(bytes)
}

fn select(bytes: &[u8], current: &str, api: &str, source: &str) -> Result<Check> {
    let release: Release =
        serde_json::from_slice(bytes).map_err(|_| "更新源没有返回有效的 Release 信息")?;
    if release.draft || release.prerelease {
        return Err("更新源没有正式版本".into());
    }
    let incoming = version(&release.tag_name)?;
    let current = version(current)?;
    if incoming <= current {
        return Ok(Check {
            update: None,
            message: format!("未发现更高版本 · {source}"),
        });
    }
    let v = format!("{}.{}.{}", incoming[0], incoming[1], incoming[2]);
    let filename = format!("ptools-{v}-windows-x64.zip");
    let asset = |name: &str| -> Result<String> {
        let matches: Vec<_> = release.assets.iter().filter(|a| a.name == name).collect();
        if matches.len() != 1 {
            return Err(format!("Release 缺少或重复附件 {name}，需要上传完整软件包"));
        }
        origin(&matches[0].browser_download_url)?;
        Ok(matches[0].browser_download_url.clone())
    };
    Ok(Check {
        update: Some(Candidate {
            version: v.clone(),
            source: source.into(),
            api: api.into(),
            package: asset(&filename)?,
            checksum: asset("SHA256SUMS.txt")?,
            filename,
        }),
        message: format!("发现 {v} · {source}"),
    })
}

fn check_source(api: &str, source: &str, current: &str) -> Result<Check> {
    let bytes = fetch(api, api, source, 2 * 1024 * 1024, 15)?;
    select(&bytes, current, api, source)
}

pub fn check(settings: &Settings, current: &str) -> Result<Check> {
    match check_source(&settings.update_lan_api, "内网", current) {
        Ok(result) => Ok(result),
        Err(lan) => match check_source(&settings.update_github_api, "GitHub", current) {
            Ok(mut result) => {
                result.message.push_str(&format!("；内网不可用：{lan}"));
                Ok(result)
            }
            Err(github) => Err(format!("内网：{lan}；GitHub：{github}")),
        },
    }
}

fn verify_checksum(bytes: &[u8], sums: &[u8], filename: &str) -> Result<()> {
    let text = std::str::from_utf8(sums).map_err(|_| "校验文件格式错误")?;
    let mut expected = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if let (Some(hash), Some(name), None) = (fields.next(), fields.next(), fields.next()) {
            if name.trim_start_matches('*') == filename {
                if expected.is_some()
                    || hash.len() != 64
                    || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err("校验文件内容错误".into());
                }
                expected = Some(hash.to_ascii_lowercase());
            }
        }
    }
    let expected = expected.ok_or("校验文件未包含完整软件包")?;
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err("软件包 SHA256 校验失败，请重试".into());
    }
    Ok(())
}

// An explicit file list protects portable data, user files, and the update helper
// itself. New release-owned files must be added here deliberately.
fn owned(name: &str) -> bool {
    REQUIRED.contains(&name)
        || matches!(
            name,
            "README.md"
                | "LICENSE"
                | "THIRD_PARTY_NOTICES.txt"
                | "plugin-development.md"
                | "native-tools.md"
                | "windows-launcher.md"
                | "quick-input.md"
                | "ui-style-guide.md"
                | "verification.md"
                | "updating.md"
                | "plugins/applications/THIRD_PARTY_NOTICES.txt"
                | "plugins/capture/THIRD_PARTY_NOTICES.txt"
        )
}

fn safe_file(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    if !owned(name)
        || name.contains(['\\', ':'])
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("软件包含有不允许的路径：{name}"));
    }
    Ok(path)
}

fn extract(bytes: Vec<u8>, stage: &Path) -> Result<Vec<String>> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "更新软件包不是有效 ZIP")?;
    if archive.len() > 100 {
        return Err("更新包文件过多".into());
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = file.name().to_owned();
        if file.is_dir() {
            if !matches!(
                name.as_str(),
                "plugins/" | "plugins/applications/" | "plugins/capture/"
            ) {
                return Err("更新包包含未知目录".into());
            }
            continue;
        }
        let relative = safe_file(&name)?;
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("更新包不能包含符号链接".into());
        }
        if files.iter().any(|n: &String| n.eq_ignore_ascii_case(&name)) {
            return Err("更新包包含重复路径".into());
        }
        total = total.checked_add(file.size()).ok_or("更新包过大")?;
        if total > MAX_EXPANDED {
            return Err("更新包解压后过大".into());
        }
        let destination = stage.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut file.by_ref().take(MAX_EXPANDED + 1), &mut output)
            .map_err(|e| e.to_string())?;
        if copied != file.size() {
            return Err("更新包文件大小不符".into());
        }
        output.sync_all().map_err(|e| e.to_string())?;
        files.push(name);
    }
    for required in REQUIRED {
        if !files.iter().any(|file| file == required) {
            return Err(format!("更新包缺少 {required}"));
        }
    }
    for id in ["applications", "capture"] {
        let plugin = load_plugin(&stage.join("plugins").join(id))?;
        if plugin.manifest.id != id {
            return Err("更新插件 ID 不匹配".into());
        }
    }
    Ok(files)
}

#[derive(Serialize, Deserialize)]
struct Plan {
    parent: u32,
    target: PathBuf,
    data: PathBuf,
    files: Vec<String>,
    version: String,
}

pub struct Prepared {
    helper: PathBuf,
    plan: PathBuf,
}

impl Prepared {
    pub fn launch(self) -> Result<()> {
        use std::os::windows::process::CommandExt;
        Command::new(&self.helper)
            .arg("--apply-update")
            .arg(&self.plan)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("无法启动更新助手：{e}"))?;
        Ok(())
    }
}

pub fn prepare(candidate: &Candidate, paths: &Paths, settings: &Settings) -> Result<Prepared> {
    let mut chosen = candidate.clone();
    let download = |c: &Candidate| -> Result<Vec<u8>> {
        let sums = fetch(&c.checksum, &c.api, &c.source, 65536, 20)?;
        let bytes = fetch(&c.package, &c.api, &c.source, MAX_PACKAGE, 180)?;
        verify_checksum(&bytes, &sums, &c.filename)?;
        Ok(bytes)
    };
    let bytes = match download(&chosen) {
        Ok(bytes) => bytes,
        Err(error) if chosen.source == "内网" => {
            let fallback = check_source(
                &settings.update_github_api,
                "GitHub",
                env!("CARGO_PKG_VERSION"),
            )?
            .update
            .ok_or_else(|| format!("内网下载失败：{error}；GitHub 没有可用新版"))?;
            if version(&fallback.version)? < version(&chosen.version)? {
                return Err(format!("内网下载失败：{error}；GitHub 版本低于待更新版本"));
            }
            chosen = fallback;
            download(&chosen)?
        }
        Err(error) => return Err(error),
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = exe
        .parent()
        .ok_or("程序目录不存在")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let work = target.join(format!(".ptools-update-{}-{}", now(), std::process::id()));
    fs::create_dir(&work)
        .map_err(|e| format!("程序目录不可写，请将 ptools 移到有写入权限的目录：{e}"))?;
    let stage = work.join("new");
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let files = extract(bytes, &stage)?;
    for name in &files {
        validate_target(&target, name)?;
    }
    // Check executable identity/version before asking the host to exit. --help
    // neither initializes plugins nor touches the user's real data directory.
    let mut child = hidden_command(&stage.join("ptools.exe"))
        .arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("新版程序验证超时，已取消更新".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or("无法读取版本")?
        .take(65536)
        .read_to_end(&mut output)
        .map_err(|e| e.to_string())?;
    if !status.success()
        || !String::from_utf8_lossy(&output).starts_with(&format!("ptools {} —", chosen.version))
    {
        return Err("更新程序的版本与 Release 不符".into());
    }
    let plan = work.join("plan.json");
    fs::write(
        &plan,
        serde_json::to_vec(&Plan {
            parent: std::process::id(),
            target,
            data: paths.root.clone(),
            files,
            version: chosen.version,
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let helper = work.join("update-helper.exe");
    fs::copy(exe, &helper).map_err(|e| e.to_string())?;
    Ok(Prepared { helper, plan })
}

fn hidden_command(exe: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(exe);
    command.creation_flags(0x08000000).stdin(Stdio::null());
    command
}

fn validate_target(target: &Path, name: &str) -> Result<()> {
    let relative = safe_file(name)?;
    let mut path = target.to_owned();
    for part in relative.components() {
        path.push(part.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("不能通过链接路径更新程序".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn replace(plan: &Plan, work: &Path) -> Result<()> {
    let backup = work.join("old");
    fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let mut changed: Vec<(&str, bool)> = Vec::new();
    let result: Result<()> = (|| {
        for name in &plan.files {
            validate_target(&plan.target, name)?;
            let target = plan.target.join(name);
            let old = backup.join(name);
            fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::create_dir_all(old.parent().unwrap()).map_err(|e| e.to_string())?;
            let existed = target.exists();
            if existed {
                fs::rename(&target, &old)
                    .map_err(|e| format!("无法替换 {name}，请关闭占用它的程序：{e}"))?;
            }
            changed.push((name, existed));
            fs::rename(work.join("new").join(name), &target).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut restore_errors = Vec::new();
        for (name, existed) in changed.into_iter().rev() {
            let target = plan.target.join(name);
            if target.exists()
                && let Err(e) = fs::remove_file(&target)
            {
                restore_errors.push(e.to_string());
                continue;
            }
            if existed && let Err(e) = fs::rename(backup.join(name), target) {
                restore_errors.push(e.to_string());
            }
        }
        return Err(if restore_errors.is_empty() {
            format!("{error}；已恢复旧版本")
        } else {
            format!(
                "{error}；部分文件恢复失败，备份位于 {}：{}",
                backup.display(),
                restore_errors.join("；")
            )
        });
    }
    Ok(())
}

pub fn apply(path: &Path) -> Result<()> {
    let work = path
        .parent()
        .ok_or("更新计划路径无效")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let plan: Plan = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let result: Result<()> = (|| {
        if work.parent() != Some(plan.target.as_path())
            || !work
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".ptools-update-")
        {
            return Err("更新工作目录不匹配".into());
        }
        if version(&plan.version)? <= version(env!("CARGO_PKG_VERSION"))? {
            return Err("不能降级更新".into());
        }
        for name in &plan.files {
            safe_file(name)?;
        }
        unsafe {
            use windows_sys::Win32::{
                Foundation::CloseHandle,
                System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
            };
            let parent = OpenProcess(PROCESS_SYNCHRONIZE, 0, plan.parent);
            if !parent.is_null() {
                let result = WaitForSingleObject(parent, 60_000);
                CloseHandle(parent);
                if result != 0 {
                    return Err("旧程序尚未退出，取消更新".into());
                }
            }
        }
        replace(&plan, &work)?;
        hidden_command(&plan.target.join("ptools.exe"))
            .arg("--hidden")
            .arg("--data-dir")
            .arg(&plan.data)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("更新完成，但重新启动失败：{e}"))?;
        Ok(())
    })();
    let report = match &result {
        Ok(()) => format!("已更新到 {}", plan.version),
        Err(e) => e.clone(),
    };
    let _ = fs::write(plan.data.join("last-update.txt"), &report);
    if let Err(error) = &result {
        crate::ui::error_box(error);
        // Restart the restored executable on replacement failure.
        let _ = hidden_command(&plan.target.join("ptools.exe"))
            .arg("--hidden")
            .arg("--data-dir")
            .arg(&plan.data)
            .spawn();
    }
    // Retain one small backup for recovery. Cleanup is deliberately separate from
    // installation and never recursively removes user data or a running helper.
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn reads_token_file_formats_without_executing_or_importing_other_secrets() {
        for input in [
            "fixture-token",
            "token:fixture-token",
            "FORGEJO-TOKEN:fixture-token",
            "TOKEN：fixture-token",
            "PTOOLS_UPDATE_TOKEN=fixture-token",
            "export FORGEJO_TOKEN='fixture-token'",
            "\u{feff}# comment\r\nGITEA_TOKEN=\"fixture-token\"\r\n",
        ] {
            assert_eq!(parse_token(input).as_deref(), Some("fixture-token"));
        }
        for input in [
            "",
            "# comment",
            "OTHER_SECRET=do-not-use",
            "TOKEN=contains whitespace",
            "Write-Host secret",
            "TOKEN=",
            "OTHER_SECRET=one\nANOTHER_SECRET=two",
        ] {
            assert!(parse_token(input).is_none());
        }
    }

    fn release(v: &str, names: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&json!({"tag_name":v,"assets":names.iter().map(|n|
            json!({"name":n,"browser_download_url":format!("https://example.com/{n}")})).collect::<Vec<_>>()})).unwrap()
    }

    #[test]
    fn release_requires_newer_stable_version_and_complete_assets() {
        for v in ["v0.1.0", "v0.1.9"] {
            assert!(
                select(&release(v, &[]), "0.1.9", "https://example.com", "内网")
                    .unwrap()
                    .update
                    .is_none()
            );
        }
        assert!(
            select(
                &release(
                    "v0.1.10",
                    &["ptools-0.1.10-windows-x64.zip", "SHA256SUMS.txt"]
                ),
                "0.1.9",
                "https://example.com",
                "内网"
            )
            .unwrap()
            .update
            .is_some()
        );
        for v in ["v0.1.10-beta.1", "0.2", "x.1.2"] {
            assert!(version(v).is_err());
        }
        assert!(
            select(
                &release("v0.1.10", &["source.zip"]),
                "0.1.9",
                "https://example.com",
                "内网"
            )
            .is_err()
        );
    }

    #[test]
    fn checksum_rejects_corruption_missing_and_duplicate_entries() {
        let good = format!("{:x}  app.zip\n", Sha256::digest(b"package"));
        assert!(verify_checksum(b"package", good.as_bytes(), "app.zip").is_ok());
        assert!(verify_checksum(b"changed", good.as_bytes(), "app.zip").is_err());
        assert!(verify_checksum(b"package", good.as_bytes(), "other.zip").is_err());
        assert!(
            verify_checksum(b"package", format!("{good}{good}").as_bytes(), "app.zip").is_err()
        );
    }

    #[test]
    fn archive_cannot_write_data_paths_or_escape_staging() {
        for name in [
            "../ptools.exe",
            "data/settings.json",
            "portable.flag",
            "C:/ptools.exe",
            "plugins/../ptools.exe",
            "plugins\\capture\\plugin.json",
        ] {
            assert!(safe_file(name).is_err());
        }
        assert!(safe_file("ptools.exe").is_ok());
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("../escaped.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        assert!(extract(archive, Path::new("not-created")).is_err());
    }

    #[test]
    fn failed_replace_restores_files_and_preserves_user_data() {
        let root = std::env::temp_dir().join(format!(
            "ptools-update-test-{}-{}",
            std::process::id(),
            now()
        ));
        let work = root.join(".ptools-update-test");
        fs::create_dir_all(work.join("new")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/settings.json"), b"preserve").unwrap();
        fs::write(root.join("ptools.exe"), b"old").unwrap();
        fs::write(work.join("new/ptools.exe"), b"new").unwrap();
        fs::write(root.join("README.md"), b"old readme").unwrap();
        let plan = Plan {
            parent: 0,
            target: root.clone(),
            data: root.join("data"),
            files: vec!["ptools.exe".into(), "README.md".into()],
            version: "0.1.10".into(),
        };
        assert!(replace(&plan, &work).unwrap_err().contains("已恢复旧版本"));
        assert_eq!(fs::read(root.join("ptools.exe")).unwrap(), b"old");
        assert_eq!(fs::read(root.join("README.md")).unwrap(), b"old readme");
        assert_eq!(
            fs::read(root.join("data/settings.json")).unwrap(),
            b"preserve"
        );
        // The exact absolute fixture path was created in this test's temp root.
        assert!(
            root.starts_with(std::env::temp_dir())
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ptools-update-test-")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_lan_falls_back_and_valid_lan_takes_priority() {
        use std::net::TcpListener;
        fn server(status: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/latest", listener.local_addr().unwrap());
            let status = status.to_owned();
            let handle = std::thread::spawn(move || {
                listener.set_nonblocking(true).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut request = [0; 4096];
                            let _ = stream.read(&mut request);
                            write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                            stream.write_all(&body).unwrap();
                            break;
                        }
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("fixture did not receive a request: {e}"),
                    }
                }
            });
            (url, handle)
        }
        let (lan, lan_thread) = server("401 Unauthorized", b"unauthorized".to_vec());
        let (github, github_thread) = server(
            "200 OK",
            release(
                "v0.1.10",
                &["ptools-0.1.10-windows-x64.zip", "SHA256SUMS.txt"],
            ),
        );
        let settings = Settings {
            update_lan_api: lan,
            update_github_api: github,
            ..Default::default()
        };
        let result = check(&settings, "0.1.9").unwrap();
        assert_eq!(result.update.unwrap().source, "GitHub");
        assert!(result.message.contains("内网不可用"));
        lan_thread.join().unwrap();
        github_thread.join().unwrap();
        let (lan, lan_thread) = server(
            "200 OK",
            release(
                "v0.1.10",
                &["ptools-0.1.10-windows-x64.zip", "SHA256SUMS.txt"],
            ),
        );
        let settings = Settings {
            update_lan_api: lan,
            update_github_api: "http://127.0.0.1:1/unreachable".into(),
            ..Default::default()
        };
        assert_eq!(
            check(&settings, "0.1.9").unwrap().update.unwrap().source,
            "内网"
        );
        lan_thread.join().unwrap();
        assert!(
            check(
                &Settings {
                    update_lan_api: "http://127.0.0.1:1/".into(),
                    update_github_api: "http://127.0.0.1:1/".into(),
                    ..Default::default()
                },
                "0.1.9"
            )
            .is_err()
        );
    }
}
