use crate::native::wide;
use ptools_core::Result;
use serde::{Deserialize, Serialize};
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::GetLastError, Networking::WinHttp::*};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: String,
    pub endpoint: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub model: String,
}

pub fn save_key(root: &std::path::Path, value: &str) -> Result<()> {
    use windows_sys::Win32::Security::Credentials::*;
    let mut target = wide(&format!(
        "ptools/capture/{}",
        root.to_string_lossy().to_lowercase()
    ));
    unsafe {
        if value.is_empty() {
            CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
            return Ok(());
        }
        if value.len() > 2560 {
            return Err("API密钥过长".into());
        }
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: value.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..std::mem::zeroed()
        };
        if CredWriteW(&credential, 0) == 0 {
            return Err("无法将翻译密钥保存到 Windows 凭据管理器".into());
        }
    }
    Ok(())
}
pub fn load_key(root: &std::path::Path) -> Option<String> {
    use windows_sys::Win32::Security::Credentials::*;
    let target = wide(&format!(
        "ptools/capture/{}",
        root.to_string_lossy().to_lowercase()
    ));
    unsafe {
        let mut credential = null_mut();
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
            return None;
        }
        let value = std::slice::from_raw_parts(
            (*credential).CredentialBlob,
            (*credential).CredentialBlobSize as usize,
        );
        let result = std::str::from_utf8(value).ok().map(str::to_owned);
        CredFree(credential.cast());
        result
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "chat".into(),
            endpoint: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}
struct Handle(*mut std::ffi::c_void);
impl Handle {
    unsafe fn new(handle: *mut std::ffi::c_void) -> Result<Self> {
        if handle.is_null() {
            Err(format!("无法连接翻译服务：{}", GetLastError()))
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

pub fn translate(settings: &Settings, text: &str) -> Result<String> {
    if settings.endpoint.is_empty() || settings.api_key.is_empty() {
        return Err("请在截图插件设置中填写翻译服务地址和密钥，AI 翻译还需要模型名称。".into());
    }
    if settings.api_key.contains(['\r', '\n']) {
        return Err("翻译密钥格式无效".into());
    }
    let chinese = text.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c));
    let target = if chinese { "EN" } else { "ZH" };
    let (body, auth) = match settings.provider.as_str() {
        "deepl" => (
            serde_json::json!({"text":[text],"target_lang":target}),
            format!("DeepL-Auth-Key {}", settings.api_key),
        ),
        "chat" if !settings.model.is_empty() => (
            serde_json::json!({"model":settings.model,"messages":[{"role":"system","content":format!("Translate the user's text into {}. Return only the translation. Preserve line breaks and numbers.",if chinese { "English" } else { "Simplified Chinese" })},{"role":"user","content":text}],"stream":false}),
            format!("Bearer {}", settings.api_key),
        ),
        "chat" => return Err("请填写翻译模型名称".into()),
        _ => return Err("翻译 provider 仅支持 chat 或 deepl".into()),
    };
    let response = post(
        &settings.endpoint,
        &auth,
        &serde_json::to_vec(&body).map_err(|e| e.to_string())?,
    )?;
    parse(&settings.provider, &response)
}
fn parse(provider: &str, bytes: &[u8]) -> Result<String> {
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "翻译服务返回的内容不是 JSON")?;
    let value = if provider == "deepl" {
        json.pointer("/translations/0/text")
    } else {
        json.pointer("/choices/0/message/content")
    };
    value
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or("翻译服务未返回译文，请检查接口和模型配置".into())
}
fn post(url: &str, authorization: &str, body: &[u8]) -> Result<Vec<u8>> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("翻译接口必须使用 HTTPS")?;
    let (host, path) = rest.split_once('/').ok_or("请填写完整翻译接口路径")?;
    if host.is_empty()
        || host.contains(['@', '\r', '\n', '?', '#'])
        || path.contains(['\r', '\n', '#'])
    {
        return Err("翻译接口地址无效".into());
    }
    let (host, port) = if let Some((host, port)) = host.rsplit_once(':') {
        (host, port.parse::<u16>().map_err(|_| "接口端口无效")?)
    } else {
        (host, 443)
    };
    unsafe {
        let session = Handle::new(WinHttpOpen(
            wide("ptools/0.1").as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 10000, 10000, 15000, 15000);
        let connection = Handle::new(WinHttpConnect(session.0, wide(host).as_ptr(), port, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            wide("POST").as_ptr(),
            wide(&format!("/{path}")).as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&redirect as *const u32).cast(),
            size_of::<u32>() as u32,
        );
        let headers = wide(&format!(
            "Content-Type: application/json\r\nAuthorization: {authorization}\r\n"
        ));
        if WinHttpSendRequest(
            request.0,
            headers.as_ptr(),
            (headers.len() - 1) as u32,
            body.as_ptr().cast(),
            body.len() as u32,
            body.len() as u32,
            0,
        ) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err(format!("翻译请求失败：{}", GetLastError()));
        }
        let mut status = 0u32;
        let mut length = 4;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut length,
            null_mut(),
        ) == 0
            || !(200..300).contains(&status)
        {
            return Err(format!(
                "翻译服务返回 HTTP {status}，请检查密钥、额度和接口地址"
            ));
        }
        let mut output = vec![];
        loop {
            let mut available = 0;
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                return Err("读取译文失败".into());
            }
            if available == 0 {
                break;
            }
            if output.len() + available as usize > 1024 * 1024 {
                return Err("翻译响应过大".into());
            }
            let offset = output.len();
            output.resize(offset + available as usize, 0);
            let mut read = 0;
            if WinHttpReadData(
                request.0,
                output[offset..].as_mut_ptr().cast(),
                available,
                &mut read,
            ) == 0
            {
                return Err("读取译文失败".into());
            }
            output.truncate(offset + read as usize);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_provider_responses_and_rejects_missing_text() {
        assert_eq!(
            parse(
                "chat",
                br#"{"choices":[{"message":{"content":"translated"}}]}"#
            )
            .unwrap(),
            "translated"
        );
        assert_eq!(
            parse("deepl", br#"{"translations":[{"text":"translated"}]}"#).unwrap(),
            "translated"
        );
        assert!(parse("chat", br#"{"error":"bad key"}"#).is_err());
    }
}
