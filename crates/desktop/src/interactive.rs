use ptools_core::*;
use std::{
    collections::HashMap,
    io::Write,
    process::{Child, Command, Stdio},
};

struct Session {
    child: Child,
    _job: PluginJob,
}

#[derive(Default)]
pub struct Sessions(HashMap<String, Session>);

impl Sessions {
    pub fn invoke(
        &mut self,
        paths: &Paths,
        settings: &Settings,
        id: &str,
        action: &str,
    ) -> Result<()> {
        self.invoke_region(paths, settings, id, action, None)
    }

    pub fn invoke_region(
        &mut self,
        paths: &Paths,
        settings: &Settings,
        id: &str,
        action: &str,
        region: Option<[i32; 4]>,
    ) -> Result<()> {
        if settings.disabled_plugins.contains(id) {
            return Err("插件已停用".into());
        }
        let plugin = load_plugin(&paths.plugins.join(id))?;
        let exe = interactive_executable(&plugin, action)?;
        let mut request = serde_json::json!({"protocol_version":2,"operation":"invoke","action":action,"data_dir":paths.root.join("plugin-data").join(id)});
        if let Some(region) = region {
            request["region"] = serde_json::json!(region);
        }
        let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        for attempt in 0..2 {
            if let Some(session) = self.0.get_mut(id)
                && session
                    .child
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_some()
            {
                self.0.remove(id);
            }
            if !self.0.contains_key(id) {
                use std::os::windows::process::CommandExt;
                let mut child = Command::new(&exe)
                    .arg("--interactive")
                    .current_dir(&plugin.directory)
                    .creation_flags(0x08000000)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("无法打开 {}：{e}", plugin.manifest.name))?;
                let job = match PluginJob::attach(&child) {
                    Ok(job) => job,
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                };
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                        child.id(),
                    );
                }
                self.0.insert(id.to_owned(), Session { child, _job: job });
            }
            let result = self
                .0
                .get_mut(id)
                .unwrap()
                .child
                .stdin
                .as_mut()
                .ok_or_else(|| "插件输入已关闭".to_owned())
                .and_then(|input| input.write_all(&bytes).map_err(|e| e.to_string()));
            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.stop(id);
                    if attempt == 1 {
                        return Err(format!("插件无法接收动作：{error}"));
                    }
                }
            }
        }
        unreachable!()
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(mut session) = self.0.remove(id) {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

impl Drop for Sessions {
    fn drop(&mut self) {
        for (_, mut session) in self.0.drain() {
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

pub fn hotkey(value: &str) -> Option<(u32, u32)> {
    let mut modifiers = 0;
    let mut key = None;
    for part in value.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => modifiers |= 2,
            "alt" => modifiers |= 1,
            "shift" => modifiers |= 4,
            "win" => modifiers |= 8,
            "space" if key.is_none() => key = Some(32),
            _ => {
                if key.is_none() && part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() {
                    key = Some(part.as_bytes()[0].to_ascii_uppercase() as u32);
                } else {
                    return None;
                }
            }
        }
    }
    if modifiers == 0 {
        None
    } else {
        key.map(|key| (modifiers | 0x4000, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotkeys_reject_multiple_primary_keys_and_require_modifiers() {
        assert_eq!(hotkey("Ctrl+1"), Some((0x4002, 49)));
        assert_eq!(hotkey("Alt+Shift+Space"), Some((0x4005, 32)));
        for invalid in ["Ctrl+A+B", "Ctrl+Space+A", "Ctrl", "A", "Ctrl+unknown"] {
            assert!(hotkey(invalid).is_none(), "{invalid}");
        }
    }
}
