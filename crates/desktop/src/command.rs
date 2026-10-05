use ptools_core::PluginJob;
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED},
    System::Pipes::PeekNamedPipe,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 1024 * 1024;

#[derive(Debug)]
pub struct CommandResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub truncated: bool,
    pub elapsed: Duration,
}

pub fn working_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|directory| directory.is_dir())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .filter(|directory| directory.is_dir())
        })
        .unwrap_or_else(std::env::temp_dir)
}

pub fn run(command: &str, cwd: &Path) -> Result<CommandResult, String> {
    run_with_limits(command, cwd, TIMEOUT, MAX_OUTPUT)
}

struct Process {
    child: Child,
    job: Option<PluginJob>,
}

impl Process {
    fn finish(&mut self) {
        // Close the Job even when PowerShell has exited: descendants may still hold pipes.
        drop(self.job.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

impl Capture {
    fn read<R: Read + AsRawHandle>(
        &mut self,
        reader: &mut R,
        stderr: bool,
        limit: usize,
    ) -> Result<usize, String> {
        let mut available = 0u32;
        let success = unsafe {
            PeekNamedPipe(
                reader.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error().map(|code| code as u32),
                Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
            ) {
                return Ok(0);
            }
            return Err(format!("无法读取命令输出：{error}"));
        }
        if available == 0 {
            return Ok(0);
        }
        let mut buffer = [0u8; 8192];
        let length = (available as usize).min(buffer.len());
        let count = reader
            .read(&mut buffer[..length])
            .map_err(|error| format!("无法读取命令输出：{error}"))?;
        let remaining = limit.saturating_sub(self.stdout.len() + self.stderr.len());
        let retain = remaining.min(count);
        let target = if stderr {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        target.extend_from_slice(&buffer[..retain]);
        self.truncated |= retain < count;
        Ok(count)
    }

    fn output(self) -> String {
        let mut output = String::from_utf8_lossy(&self.stdout).into_owned();
        if !self.stderr.is_empty() {
            let errors = stderr_text(&String::from_utf8_lossy(&self.stderr));
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&errors);
        }
        // Win32 text controls and CF_UNICODETEXT use NUL as a string terminator.
        output.replace('\0', "␀")
    }
}

fn stderr_text(value: &str) -> String {
    // Windows PowerShell uses CLIXML for redirected EncodedCommand error streams,
    // even with OutputFormat Text. Its stream text is serialized in S elements.
    let Some(marker) = value.find("#< CLIXML") else {
        return value.into();
    };
    let document = &value[marker..];
    if !document.contains("<Objs ") {
        return value.into();
    }
    let mut output = value[..marker].to_owned();
    let mut cursor = 0;
    let mut messages = 0;
    while let Some(offset) = document[cursor..].find("<S ") {
        let start = cursor + offset;
        let Some(tag_end) = document[start..].find('>').map(|offset| start + offset) else {
            break;
        };
        let Some(end) = document[tag_end + 1..]
            .find("</S>")
            .map(|offset| tag_end + 1 + offset)
        else {
            break;
        };
        if document[start..tag_end].contains(" S=\"") {
            let message = decode_xml_text(&document[tag_end + 1..end]);
            output.push_str(&message);
            if !message.ends_with('\n') {
                output.push('\n');
            }
            messages += 1;
        }
        cursor = end + 4;
    }
    if messages > 0 || document.contains(" S=\"progress\"") {
        output
    } else {
        value.into()
    }
}

fn decode_xml_text(value: &str) -> String {
    let value = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let encoded: Vec<u16> = value.encode_utf16().collect();
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut cursor = 0;
    while cursor < encoded.len() {
        let escape = encoded.get(cursor..cursor + 7).and_then(|escape| {
            if escape[0] != b'_' as u16 || escape[1] != b'x' as u16 || escape[6] != b'_' as u16 {
                return None;
            }
            let mut unit = 0u16;
            for digit in &escape[2..6] {
                unit = (unit << 4) | char::from_u32(*digit as u32)?.to_digit(16)? as u16;
            }
            Some(unit)
        });
        if let Some(unit) = escape {
            decoded.push(unit);
            cursor += 7;
        } else {
            decoded.push(encoded[cursor]);
            cursor += 1;
        }
    }
    String::from_utf16_lossy(&decoded)
}

fn powershell() -> Result<PathBuf, String> {
    let directory = std::env::var_os("SystemRoot").ok_or("找不到 Windows 系统目录")?;
    let executable =
        PathBuf::from(directory).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    if !executable.is_absolute() || !executable.is_file() {
        return Err("找不到 Windows PowerShell".into());
    }
    Ok(executable)
}

fn encoded_command(command: &str) -> String {
    // stdin is a startup gate: no user code runs before the host attaches the Job.
    let script = format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false);\n\
         [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false);\n\
         $OutputEncoding = [Console]::OutputEncoding;\n\
         $ProgressPreference = 'SilentlyContinue';\n\
         if ([Console]::In.ReadLine() -ne 'ptools-run') {{ exit 125 }};\n\
         $global:LASTEXITCODE = 0;\n\
         $script:ptoolsCommandLastSucceeded = $true;\n\
         & {{\n{command}\n$script:ptoolsCommandLastSucceeded = $?\n}};\n\
         $ptoolsInvocationSucceeded = $?;\n\
         if (-not $ptoolsInvocationSucceeded -or -not $script:ptoolsCommandLastSucceeded) {{\n\
             if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}; exit 1\n\
         }};\n\
         exit 0"
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as usize;
        let b = chunk.get(1).copied().unwrap_or(0) as usize;
        let c = chunk.get(2).copied().unwrap_or(0) as usize;
        encoded.push(ALPHABET[a >> 2] as char);
        encoded.push(ALPHABET[((a & 3) << 4) | (b >> 4)] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[c & 63] as char
        } else {
            '='
        });
    }
    encoded
}

fn run_with_limits(
    command: &str,
    cwd: &Path,
    timeout: Duration,
    output_limit: usize,
) -> Result<CommandResult, String> {
    if command.trim().is_empty() || command.contains('\0') {
        return Err("请输入有效命令".into());
    }
    if !cwd.is_dir() {
        return Err("命令工作目录不存在".into());
    }
    let encoded = encoded_command(command);
    if encoded.len() > 30000 {
        return Err("命令过长，请缩短命令后重试".into());
    }
    let started = Instant::now();
    let mut child = Command::new(powershell()?)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-InputFormat",
            "Text",
            "-OutputFormat",
            "Text",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 Windows PowerShell：{error}"))?;
    let job = match PluginJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法管理命令进程：{error}"));
        }
    };
    let mut process = Process {
        child,
        job: Some(job),
    };
    let mut stdout = process.child.stdout.take().ok_or("无法读取命令输出")?;
    let mut stderr = process.child.stderr.take().ok_or("无法读取命令错误输出")?;
    let mut input = process.child.stdin.take().ok_or("无法初始化命令输入")?;
    input
        .write_all(b"ptools-run\n")
        .map_err(|error| format!("无法初始化命令输入：{error}"))?;
    drop(input);
    let mut capture = Capture::default();
    let mut exit_code = None;
    let mut timed_out = false;
    loop {
        let read = capture.read(&mut stdout, false, output_limit)?
            + capture.read(&mut stderr, true, output_limit)?;
        if let Some(status) = process
            .child
            .try_wait()
            .map_err(|error| format!("无法读取命令退出状态：{error}"))?
        {
            exit_code = status.code();
            break;
        }
        if capture.truncated {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break;
        }
        if read == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    process.finish();
    // Only read bytes already available; no reader can block on inherited handles.
    while !capture.truncated {
        let read = capture.read(&mut stdout, false, output_limit)?
            + capture.read(&mut stderr, true, output_limit)?;
        if read == 0 {
            break;
        }
    }
    let truncated = capture.truncated;
    Ok(CommandResult {
        output: capture.output(),
        exit_code,
        timed_out,
        truncated,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_multiline_chinese_stdout_stderr_and_exit_code() {
        let result = run(
            "Write-Output '第一行'; Write-Output 'second'; [Console]::Error.WriteLine('标准错误'); exit 7",
            &working_directory(),
        )
        .unwrap();
        assert!(result.output.contains("第一行"), "{}", result.output);
        assert!(result.output.contains("second"), "{}", result.output);
        assert!(result.output.contains("标准错误"), "{}", result.output);
        assert_eq!(result.exit_code, Some(7));
        assert!(!result.timed_out);
        assert!(!result.truncated);
    }

    #[test]
    fn preserves_chinese_text_after_nul_output() {
        let result = run(
            "[Console]::Out.Write('中文前' + [char]0 + '中文后')",
            &working_directory(),
        )
        .unwrap();
        assert_eq!(result.output, "中文前␀中文后");
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn captures_powershell_errors_as_text_and_failure() {
        let result = run("Write-Error '命令失败'", &working_directory()).unwrap();
        assert!(result.output.contains("命令失败"), "{}", result.output);
        assert!(!result.output.contains("#< CLIXML"), "{}", result.output);
        assert_eq!(result.exit_code, Some(1));
    }

    #[test]
    fn supports_powershell_and_native_commands() {
        let result = run(
            "Get-Date -Format yyyy; Get-Process -Id $PID | Select-Object -ExpandProperty Id; & \"$env:SystemRoot\\System32\\cmd.exe\" /d /c echo native-output 中文输出",
            &working_directory(),
        )
        .unwrap();
        assert!(result.output.contains("native-output"), "{}", result.output);
        assert!(result.output.contains("中文输出"), "{}", result.output);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.output.lines().count() >= 3);
    }

    #[test]
    fn propagates_native_command_failure() {
        let result = run(
            "& \"$env:SystemRoot\\System32\\cmd.exe\" /d /c exit 9",
            &working_directory(),
        )
        .unwrap();
        assert_eq!(result.exit_code, Some(9), "{}", result.output);
    }

    #[test]
    fn times_out_and_returns_without_waiting_for_sleep() {
        let result = run_with_limits(
            "Start-Sleep -Seconds 20",
            &working_directory(),
            Duration::from_millis(700),
            MAX_OUTPUT,
        )
        .unwrap();
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
        assert!(result.elapsed < Duration::from_secs(5));
    }

    #[test]
    fn limits_total_output_and_stops_a_continuous_writer() {
        let result = run_with_limits(
            "while ($true) { [Console]::Out.WriteLine('abcdefghijklmno') }",
            &working_directory(),
            Duration::from_secs(5),
            1024,
        )
        .unwrap();
        assert!(result.truncated);
        assert!(!result.timed_out);
        assert!(result.output.len() <= 1024);
        assert!(result.elapsed < Duration::from_secs(5));
    }

    #[test]
    fn normal_exit_reaps_descendants_that_inherit_output_handles() {
        let result = run(
            "Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 20' -NoNewWindow -PassThru | Select-Object -ExpandProperty Id",
            &working_directory(),
        )
        .unwrap();
        assert_eq!(result.exit_code, Some(0), "{}", result.output);
        assert!(result.elapsed < Duration::from_secs(5));
        let pid: u32 = result.output.trim().parse().unwrap();
        unsafe {
            use windows_sys::Win32::{
                Foundation::CloseHandle,
                System::Threading::{
                    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                },
            };
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !handle.is_null() {
                let mut code = 0;
                let read = GetExitCodeProcess(handle, &mut code);
                CloseHandle(handle);
                assert_ne!(read, 0);
                assert_ne!(code, 259, "descendant remains active after the Job closes");
            }
        }
    }

    #[test]
    fn rejects_invalid_input_before_starting_a_process() {
        assert!(run("", &working_directory()).is_err());
        assert!(run("Write-Output '\0'", &working_directory()).is_err());
        assert!(run("Get-Date", Path::new("Z:\\ptools-missing-directory-209843")).is_err());
    }

    #[test]
    fn decodes_powershell_stream_escapes_without_decoding_twice() {
        assert_eq!(
            stderr_text(
                "prefix\n#< CLIXML\n<Objs Version=\"1.1\"><S S=\"Error\">中文&lt;&amp;&gt;_x000D__x000A__x005F_x000A_</S></Objs>"
            ),
            "prefix\n中文<&>\r\n_x000A_\n"
        );
        assert_eq!(stderr_text("plain standard error"), "plain standard error");
    }
}
