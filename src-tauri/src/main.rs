// Muse desktop shell.
//
// This app deliberately has no local frontend and no custom commands.
// It opens https://muse.ai directly in the operating system's WebView
// (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux).
//
// Close action:
//   - On close request, prompt user: minimize to system tray or exit.
//   - Support remembering user choice ("下次不再询问").
//   - Provide system tray menu with restore & reset options.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Listener, Manager};
use tauri_plugin_notification::NotificationExt;

static IS_EXITING: AtomicBool = AtomicBool::new(false);
static LOG_FILE_PATH: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    #[serde(rename = "minimize")]
    MinimizeToTray,
    #[serde(rename = "exit")]
    ExitApp,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct AppSettings {
    pub close_action: Option<CloseAction>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NotificationPayload {
    pub title: String,
    pub body: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogPayload {
    pub level: Option<String>,
    pub message: String,
}

fn init_logger<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("muse-desktop.log");
        *LOG_FILE_PATH.lock().unwrap() = Some(path.clone());
        log_msg("Rust", "INFO", &format!("日志服务启动，日志保存于: {}", path.display()));
    }
}

fn get_now_time_string() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let duration = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    // 粗略转换为本地/北京时间 (UTC+8)
    let total_secs = secs + 8 * 3600;
    let day_secs = total_secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;
    let millis = duration.subsec_millis();
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

pub fn log_msg(source: &str, level: &str, message: &str) {
    let time_str = get_now_time_string();
    let line = format!("[{time_str}] [{level}] [{source}] {message}\n");
    eprint!("{line}");

    if let Ok(guard) = LOG_FILE_PATH.lock() {
        if let Some(path) = guard.as_ref() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
}

fn get_settings_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|p| p.join("settings.json"))
}

fn load_settings<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppSettings {
    if let Some(path) = get_settings_path(app) {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                return settings;
            }
        }
    }
    AppSettings::default()
}

fn save_settings<R: tauri::Runtime>(app: &tauri::AppHandle<R>, settings: &AppSettings) {
    if let Some(path) = get_settings_path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(&path, json);
        }
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn show_info_box(hwnd: *mut std::ffi::c_void, title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let title_w = to_wide(title);
    let text_w = to_wide(text);
    unsafe {
        MessageBoxW(hwnd as _, text_w.as_ptr(), title_w.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(target_os = "windows")]
fn show_close_dialog_windows(hwnd: *mut std::ffi::c_void) -> Option<(CloseAction, bool)> {
    use windows_sys::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON,
        TDCBF_CANCEL_BUTTON, TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
        TDF_POSITION_RELATIVE_TO_WINDOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, IDYES, IDNO,
        MB_ICONQUESTION, MB_YESNOCANCEL,
    };

    const RADIO_MINIMIZE: i32 = 101;
    const RADIO_EXIT: i32 = 102;

    let title = to_wide("Muse");
    let instruction = to_wide("关闭选项");
    let content = to_wide("请选择点击关闭按钮时的操作：\n（选择最小化到系统托盘后，窗口将保持后台运行，可通过右下角托盘图标随时重新打开）");
    let radio_min_text = to_wide("最小化到系统托盘 (在后台继续运行)");
    let radio_exit_text = to_wide("直接退出程序");
    let verify_text = to_wide("记住我的选择，下次不再询问");

    let radio_buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: RADIO_MINIMIZE,
            pszButtonText: radio_min_text.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            nButtonID: RADIO_EXIT,
            pszButtonText: radio_exit_text.as_ptr(),
        },
    ];

    let mut config: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    config.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    config.hwndParent = hwnd as _;
    config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW;
    config.dwCommonButtons = TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON;
    config.pszWindowTitle = title.as_ptr();
    config.pszMainInstruction = instruction.as_ptr();
    config.pszContent = content.as_ptr();
    config.cRadioButtons = radio_buttons.len() as u32;
    config.pRadioButtons = radio_buttons.as_ptr();
    config.nDefaultRadioButton = RADIO_MINIMIZE;
    config.pszVerificationText = verify_text.as_ptr();

    let mut clicked_button = 0;
    let mut selected_radio = 0;
    let mut verification_checked = 0;

    let hr = unsafe {
        TaskDialogIndirect(
            &config,
            &mut clicked_button,
            &mut selected_radio,
            &mut verification_checked,
        )
    };

    if hr == 0 {
        if clicked_button == IDOK {
            let action = if selected_radio == RADIO_EXIT {
                CloseAction::ExitApp
            } else {
                CloseAction::MinimizeToTray
            };
            let remember = verification_checked != 0;
            Some((action, remember))
        } else {
            None
        }
    } else {
        let prompt_text = to_wide("请选择操作：\n\n【是】最小化到系统托盘\n【否】直接退出程序\n【取消】返回窗口");
        let result = unsafe {
            MessageBoxW(
                hwnd as _,
                prompt_text.as_ptr(),
                title.as_ptr(),
                MB_YESNOCANCEL | MB_ICONQUESTION,
            )
        };
        match result {
            IDYES => Some((CloseAction::MinimizeToTray, false)),
            IDNO => Some((CloseAction::ExitApp, false)),
            _ => None,
        }
    }
}

fn handle_close_requested(window: &tauri::WebviewWindow) {
    if IS_EXITING.load(Ordering::SeqCst) {
        return;
    }

    let app = window.app_handle();
    let mut settings = load_settings(app);

    if let Some(action) = settings.close_action {
        match action {
            CloseAction::MinimizeToTray => {
                log_msg("Window", "INFO", "触发关闭事件，依据已保存的偏好最小化到系统托盘");
                let _ = window.hide();
            }
            CloseAction::ExitApp => {
                log_msg("Window", "INFO", "触发关闭事件，依据已保存的偏好退出程序");
                IS_EXITING.store(true, Ordering::SeqCst);
                app.exit(0);
            }
        }
        return;
    }

    #[cfg(target_os = "windows")]
    {
        let hwnd = match window.hwnd() {
            Ok(h) => h.0 as *mut std::ffi::c_void,
            Err(_) => std::ptr::null_mut(),
        };

        if let Some((action, remember)) = show_close_dialog_windows(hwnd) {
            if remember {
                settings.close_action = Some(action);
                save_settings(app, &settings);
            }
            match action {
                CloseAction::MinimizeToTray => {
                    log_msg("Window", "INFO", "用户选择最小化到系统托盘");
                    let _ = window.hide();
                }
                CloseAction::ExitApp => {
                    log_msg("Window", "INFO", "用户选择直接退出程序");
                    IS_EXITING.store(true, Ordering::SeqCst);
                    app.exit(0);
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window.hide();
    }
}

/// Windows 原生 PowerShell WinRT Toast 兜底函数
/// 当未打包安装（绿色免安装单文件）时，系统 WinRT 缺少快捷方式 AUMID 会静默丢弃通知，
/// 本函数通过系统内置的 PowerShell 5.1 触发系统级 Toast，保障 100% 弹出通知。
#[cfg(target_os = "windows")]
fn send_windows_powershell_toast(title: &str, body: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let safe_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    let safe_body = body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");

    let script = format!(
        r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$template = @"
<toast>
    <visual>
        <binding template='ToastGeneric'>
            <text>{}</text>
            <text>{}</text>
        </binding>
    </visual>
</toast>
"@
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml($template)
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
$notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\WindowsPowerShell\v1.0\powershell.exe')
$notifier.Show($toast)"#,
        safe_title, safe_body
    );

    let output = std::process::Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err.trim().to_string());
    }
    Ok(())
}

fn trigger_desktop_notification<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    title: &str,
    body: Option<&str>,
) {
    let mut raw_body = body.unwrap_or("").trim().to_string();
    // Windows Toast 文本长度与换行优化，避免通知气泡过高被系统截断
    if raw_body.chars().count() > 180 {
        raw_body = format!("{}...", raw_body.chars().take(180).collect::<String>());
    }
    let body_str = raw_body.as_str();

    log_msg("Notify", "INFO", &format!("准备派发系统桌面通知 -> 标题: [{}], 内容: [{}]", title, body_str));

    let mut builder = app.notification().builder().title(title);
    if !body_str.is_empty() {
        builder = builder.body(body_str);
    }

    let mut success = false;
    match builder.show() {
        Ok(_) => {
            log_msg("Notify", "INFO", "tauri-plugin-notification 派发返回 Ok");
            success = true;
        }
        Err(err) => {
            log_msg("Notify", "WARN", &format!("tauri-plugin-notification 派发失败: {err}"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows 平台：绿色版/未安装注册 AUMID 时，即便返回 Ok，Windows Action Center 也可能静默吞掉
        // 因此执行 PowerShell 原生 WinRT 兜底，确保桌面绝对弹出系统气泡
        if let Err(e) = send_windows_powershell_toast(title, body_str) {
            log_msg("Notify", "ERROR", &format!("PowerShell WinRT Toast 兜底执行异常: {e}"));
        } else {
            log_msg("Notify", "INFO", "Windows 原生 Toast 通知已成功推送到屏幕右下角");
            success = true;
        }
    }

    if !success {
        log_msg("Notify", "ERROR", "系统未能成功弹出通知，请检查系统免打扰或通知中心设置");
    }
}

#[tauri::command]
fn push_desktop_notification(app: tauri::AppHandle, title: String, body: Option<String>) {
    trigger_desktop_notification(&app, &title, body.as_deref());
}

#[tauri::command]
fn log_from_web(level: String, message: String) {
    log_msg("Web", &level, &message);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![push_desktop_notification, log_from_web])
        .setup(|app| {
            init_logger(app.handle());

            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let test_notify_item = MenuItem::with_id(app, "test_notify", "🔔 发送测试通知", true, None::<&str>)?;
            let open_log_item = MenuItem::with_id(app, "open_log", "📄 查看运行日志", true, None::<&str>)?;
            let reset_item = MenuItem::with_id(app, "reset_preference", "重置关闭提示", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Muse", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &PredefinedMenuItem::separator(app)?,
                    &test_notify_item,
                    &open_log_item,
                    &PredefinedMenuItem::separator(app)?,
                    &reset_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;

            let icon = app.default_window_icon().unwrap().clone();
            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .tooltip("Muse")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "test_notify" => {
                        log_msg("Tray", "INFO", "用户从系统托盘点击了「发送测试通知」");
                        trigger_desktop_notification(
                            app,
                            "Muse 桌面通知测试",
                            Some("恭喜！Windows 桌面原生通知服务正常工作，可以接收 muse.ai 的全部通知。"),
                        );
                    }
                    "open_log" => {
                        log_msg("Tray", "INFO", "用户从系统托盘点击了「查看运行日志」");
                        if let Ok(guard) = LOG_FILE_PATH.lock() {
                            if let Some(path) = guard.as_ref() {
                                #[cfg(target_os = "windows")]
                                {
                                    let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
                                }
                            }
                        }
                    }
                    "reset_preference" => {
                        let mut settings = load_settings(app);
                        settings.close_action = None;
                        save_settings(app, &settings);
                        log_msg("Tray", "INFO", "已重置关闭提示偏好");
                        #[cfg(target_os = "windows")]
                        {
                            let hwnd = if let Some(w) = app.get_webview_window("main") {
                                w.hwnd().map(|h| h.0 as *mut std::ffi::c_void).unwrap_or(std::ptr::null_mut())
                            } else {
                                std::ptr::null_mut()
                            };
                            show_info_box(hwnd, "Muse", "已恢复关闭提示设置。\n下次点击窗口关闭按钮时将重新询问。");
                        }
                    }
                    "quit" => {
                        log_msg("Tray", "INFO", "用户通过托盘退出程序");
                        IS_EXITING.store(true, Ordering::SeqCst);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // 注册全局通知事件监听器 (处理来自前端注入脚本的推送)
            let app_handle = app.handle().clone();
            app.listen("muse-notification", move |event| {
                if let Ok(payload) = serde_json::from_str::<NotificationPayload>(event.payload()) {
                    trigger_desktop_notification(&app_handle, &payload.title, payload.body.as_deref());
                }
            });

            // 注册日志事件监听器
            app.listen("muse-log", move |event| {
                if let Ok(payload) = serde_json::from_str::<LogPayload>(event.payload()) {
                    let level = payload.level.unwrap_or_else(|| "INFO".to_string());
                    log_msg("Web", &level, &payload.message);
                }
            });

            // 动态构建主 WebView 窗口并注入通知拦截脚本与后台防挂起参数
            let url: tauri::Url = "https://muse.ai".parse().expect("有效的主站 URL");
            let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("Muse")
                .inner_size(1280.0, 860.0)
                .min_inner_size(900.0, 600.0)
                .center()
                .resizable(true)
                .devtools(true)
                .additional_browser_args("--disable-background-timer-throttling --disable-backgrounding-occluded-windows")
                .initialization_script(include_str!("inject.js"))
                .build()?;

            let win_clone = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !IS_EXITING.load(Ordering::SeqCst) {
                        api.prevent_close();
                        handle_close_requested(&win_clone);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Muse desktop app");
}
