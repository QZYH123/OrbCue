//! Opt-in ntfy POST for the same cue as a system toast.
//! The topic URL is a secret: do not log it.

use orbcue_core::{
    normalize_phone_notify_url, phone_notify_for_attention, phone_notify_request, Attention,
    PhoneNotifyRequest,
};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

static PHONE_NOTIFY_URL: Mutex<String> = Mutex::new(String::new());

fn url_slot() -> std::sync::MutexGuard<'static, String> {
    PHONE_NOTIFY_URL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn phone_notify_path() -> PathBuf {
    orbcue_ipc::default_state_path().with_file_name("phone-notify.url")
}

pub fn load() {
    let Ok(text) = fs::read_to_string(phone_notify_path()) else {
        return;
    };
    let Ok(Some(url)) = normalize_phone_notify_url(&text) else {
        if !text.trim().is_empty() {
            eprintln!("OrbCue: ignored invalid phone notify address");
        }
        return;
    };
    *url_slot() = url;
}

#[tauri::command]
pub fn phone_notify_url() -> String {
    url_slot().clone()
}

#[tauri::command]
pub fn set_phone_notify_url(url: String) -> Result<String, String> {
    let url = match normalize_phone_notify_url(&url) {
        Ok(url) => url,
        Err(error) => return Err(error.message().to_owned()),
    };
    let path = phone_notify_path();
    match &url {
        None => match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(phone_url_save_error(error)),
        },
        Some(url) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(phone_url_save_error)?;
            }
            fs::write(&path, url).map_err(phone_url_save_error)?;
        }
    }
    let stored = url.unwrap_or_default();
    *url_slot() = stored.clone();
    Ok(stored)
}

fn phone_url_save_error(error: std::io::Error) -> String {
    orbcue_core::t!(
        "写不了手机提醒地址：{error}",
        "Couldn't save the phone URL: {error}"
    )
}

#[tauri::command]
pub fn preview_phone_notify() -> Result<(), String> {
    let url = url_slot().clone();
    let Some(request) = phone_notify_request(
        &url,
        orbcue_core::pick("手机提醒已打开", "Phone alerts are on"),
        orbcue_core::pick(
            "等待输入、授权、失败或完成时会再发一次",
            "Another alert is sent for input, approval, failure, or completion.",
        ),
    )
    .map_err(|error| error.message().to_owned())?
    else {
        return Err(orbcue_core::t!("先填写话题地址", "Enter a topic URL first"));
    };
    post(&request)
}

pub fn queue(attention: Option<&Attention>, notify_completion: bool, project_path: Option<&str>) {
    let Some(attention) = attention else {
        return;
    };
    let url = url_slot().clone();
    if url.is_empty() {
        return;
    }
    let request = match phone_notify_for_attention(&url, attention, notify_completion, project_path)
    {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(error) => {
            eprintln!("OrbCue: phone notify skipped: {}", error.message());
            return;
        }
    };
    if std::thread::Builder::new()
        .name("phone-notify".to_owned())
        .spawn(move || {
            if let Err(error) = post(&request) {
                eprintln!("OrbCue: phone notify failed: {error}");
            }
        })
        .is_err()
    {
        eprintln!(
            "{}",
            orbcue_core::pick(
                "OrbCue: phone notify failed: 发不出去",
                "OrbCue: phone notify failed: couldn't send"
            )
        );
    }
}

fn post(request: &PhoneNotifyRequest) -> Result<(), String> {
    unsafe { post_winhttp(request) }
}

unsafe fn post_winhttp(request: &PhoneNotifyRequest) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
        WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    struct Internet(*mut core::ffi::c_void);
    impl Drop for Internet {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }
    impl Internet {
        fn open(handle: *mut core::ffi::c_void) -> Result<Self, String> {
            if handle.is_null() {
                Err(winhttp_error())
            } else {
                Ok(Self(handle))
            }
        }
    }

    let agent = Wide::new("OrbCue");
    let session = Internet::open(WinHttpOpen(
        agent.as_pcwstr(),
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        PCWSTR::null(),
        PCWSTR::null(),
        0,
    ))?;
    WinHttpSetTimeouts(session.0, 8_000, 8_000, 8_000, 8_000).map_err(error_text)?;

    let host = if request.host.contains(':') {
        format!("[{}]", request.host)
    } else {
        request.host.clone()
    };
    let host = Wide::new(&host);
    let connect = Internet::open(WinHttpConnect(session.0, host.as_pcwstr(), request.port, 0))?;
    let path = Wide::new(&request.path);
    let flags = if request.https {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let verb = Wide::new("POST");
    let http = Internet::open(WinHttpOpenRequest(
        connect.0,
        verb.as_pcwstr(),
        path.as_pcwstr(),
        PCWSTR::null(),
        PCWSTR::null(),
        core::ptr::null(),
        flags,
    ))?;
    let redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
    WinHttpSetOption(
        Some(http.0.cast_const()),
        WINHTTP_OPTION_REDIRECT_POLICY,
        Some(&redirect),
    )
    .map_err(error_text)?;

    let headers = "Content-Type: application/json; charset=utf-8\r\n"
        .encode_utf16()
        .collect::<Vec<_>>();
    let body = request.body.as_bytes();
    let len = u32::try_from(body.len())
        .map_err(|_| orbcue_core::t!("消息太长", "The message is too long"))?;
    WinHttpSendRequest(
        http.0,
        Some(headers.as_slice()),
        Some(body.as_ptr().cast()),
        len,
        len,
        0,
    )
    .map_err(error_text)?;
    WinHttpReceiveResponse(http.0, core::ptr::null_mut()).map_err(error_text)?;

    let mut status = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        http.0,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some((&mut status as *mut u32).cast()),
        &mut len,
        &mut index,
    )
    .map_err(error_text)?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(match status {
            401 | 403 => orbcue_core::t!("这个话题拒绝发送", "This topic refused the message"),
            404 => orbcue_core::t!("话题地址不对", "The topic URL looks wrong"),
            _ => orbcue_core::t!("ntfy 返回 {status}", "ntfy returned {status}"),
        })
    }
}

fn winhttp_error() -> String {
    error_text(windows::core::Error::from_win32())
}

fn error_text(error: windows::core::Error) -> String {
    let message = error.message();
    let message = message.trim();
    if message.is_empty() {
        orbcue_core::t!("连不上", "Couldn't connect")
    } else {
        message.to_owned()
    }
}

struct Wide(Vec<u16>);

impl Wide {
    fn new(value: &str) -> Self {
        Self(value.encode_utf16().chain(std::iter::once(0)).collect())
    }

    fn as_pcwstr(&self) -> windows::core::PCWSTR {
        windows::core::PCWSTR(self.0.as_ptr())
    }
}
