use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PROTOCOL_VERSION: u32 = 1;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub runtime: Runtime,
    #[serde(default)]
    pub executable: Option<String>,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub actions: Vec<PluginAction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginAction {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub hotkey: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Config,
    Executable,
    Interactive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub target: Target,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Path { path: String },
    Url { url: String },
    App { app_id: String },
    Plugin { action: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexRequest {
    pub protocol_version: u32,
    pub operation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexResponse {
    pub protocol_version: u32,
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    pub plugins: BTreeMap<String, Vec<Entry>>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    pub disabled_plugins: BTreeSet<String>,
    pub usage: BTreeMap<String, Usage>,
    #[serde(default)]
    pub plugin_hotkeys: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".into(),
            disabled_plugins: BTreeSet::new(),
            usage: BTreeMap::new(),
            plugin_hotkeys: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub count: u32,
    pub last_used: u64,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn validate_entry(entry: &Entry) -> Result<()> {
    if entry.id.is_empty()
        || entry.id.len() > 1024
        || entry.title.trim().is_empty()
        || entry.title.len() > 1024
    {
        return Err("条目 ID 或名称为空或过长".into());
    }
    if entry.subtitle.len() > 8192
        || entry.keywords.len() > 64
        || entry.keywords.iter().any(|x| x.len() > 1024)
    {
        return Err("条目描述或关键词过长".into());
    }
    let target = match &entry.target {
        Target::Path { path } => {
            if !std::path::Path::new(path).is_absolute() {
                return Err("启动路径必须是绝对路径".into());
            }
            path
        }
        Target::Url { url } => {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err("网址只支持 http/https".into());
            }
            url
        }
        Target::App { app_id } => {
            if app_id.is_empty() || app_id.contains(['/', '\\', '"', '\r', '\n']) {
                return Err("无效的应用标识".into());
            }
            app_id
        }
        Target::Plugin { action } => {
            if action.is_empty()
                || action.len() > 64
                || !action
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err("无效的插件动作".into());
            }
            action
        }
    };
    if target.is_empty() || target.len() > 32760 || target.contains('\0') {
        return Err("无效的启动目标".into());
    }
    Ok(())
}
