use crate::Result;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub plugins: PathBuf,
    pub portable: bool,
}

impl Paths {
    pub fn resolve(exe_dir: &Path, portable: bool, override_root: Option<PathBuf>) -> Result<Self> {
        let portable = portable || exe_dir.join("portable.flag").exists();
        let root = override_root.unwrap_or_else(|| {
            if portable {
                exe_dir.join("data")
            } else {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| exe_dir.to_owned())
                    .join("ptools")
            }
        });
        let root = std::path::absolute(root).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("plugins"))
            .map_err(|e| format!("无法创建数据目录 {}: {e}", root.display()))?;
        Ok(Self {
            plugins: root.join("plugins"),
            root,
            portable,
        })
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("index.json")
    }
}

pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("{} 格式错误：{e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("无法读取 {}: {e}", path.display())),
    }
}

pub fn write_json<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    // Keep a readable previous version, then atomically replace the destination.
    let backup = path.with_extension("json.bak");
    if backup.exists() {
        fs::remove_file(&backup).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        fs::copy(path, &backup).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        for attempt in 0..10 {
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } != 0
            {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if attempt == 9 || !matches!(error.raw_os_error(), Some(5 | 32 | 33)) {
                let _ = fs::remove_file(&temp);
                return Err(error.to_string());
            }
            // A reader or antivirus can briefly deny replacement while sharing reads.
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::now;
    #[test]
    fn atomic_replace_survives_a_short_lived_windows_reader() {
        use std::os::windows::fs::OpenOptionsExt;
        let root =
            std::env::temp_dir().join(format!("ptools-storage-{}-{}", std::process::id(), now()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.json");
        write_json(&path, &vec!["old"]).unwrap();
        let reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let target = path.clone();
        let writer = std::thread::spawn(move || write_json(&target, &vec!["new"]));
        std::thread::sleep(std::time::Duration::from_millis(75));
        drop(reader);
        writer.join().unwrap().unwrap();
        assert_eq!(read_json::<Vec<String>>(&path).unwrap(), ["new"]);
        fs::remove_dir_all(root).unwrap();
    }
}
