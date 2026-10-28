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

use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

static IS_EXITING: AtomicBool = AtomicBool::new(false);

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
        // Fallback: 如果 TaskDialog 失败，使用基础 MessageBoxW 兜底，避免无响应
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
                let _ = window.hide();
            }
            CloseAction::ExitApp => {
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
                    let _ = window.hide();
                }
                CloseAction::ExitApp => {
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

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let reset_item = MenuItem::with_id(app, "reset_preference", "重置关闭提示", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Muse", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_item,
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
                    "reset_preference" => {
                        let mut settings = load_settings(app);
                        settings.close_action = None;
                        save_settings(app, &settings);
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

            if let Some(window) = app.get_webview_window("main") {
                let win_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !IS_EXITING.load(Ordering::SeqCst) {
                            api.prevent_close();
                            handle_close_requested(&win_clone);
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Muse desktop app");
}