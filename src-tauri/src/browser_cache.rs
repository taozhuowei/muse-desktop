use tauri::{Manager, Webview};

#[tauri::command]
pub async fn clear_browser_cache(webview: Webview) -> Result<(), String> {
    if webview.label() != "shell" {
        return Err("只有 shell 窗口可以清理浏览器缓存".into());
    }

    #[cfg(target_os = "windows")]
    {
        use webview2_com::{
            ClearBrowsingDataCompletedHandler,
            Microsoft::Web::WebView2::Win32::{
                ICoreWebView2Profile2, ICoreWebView2_13,
                COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE,
            },
        };
        use windows::core::Interface;

        let main = webview
            .app_handle()
            .get_webview("main")
            .ok_or_else(|| "找不到 main WebView".to_string())?;
        let (sender, receiver) = std::sync::mpsc::channel();
        main.with_webview(move |webview| {
            let callback_sender = sender.clone();
            let result = unsafe {
                (|| -> windows::core::Result<()> {
                    let webview = webview.controller().CoreWebView2()?;
                    let profile = webview.cast::<ICoreWebView2_13>()?.Profile()?;
                    let profile = profile.cast::<ICoreWebView2Profile2>()?;
                    let callback =
                        ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
                            let _ = callback_sender.send(result.map_err(|e| e.to_string()));
                            Ok(())
                        }));
                    profile
                        .ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, &callback)
                })()
            };
            if let Err(error) = result {
                let _ = sender.send(Err(error.to_string()));
            }
        })
        .map_err(|e| format!("无法访问 main WebView: {e}"))?;
        tauri::async_runtime::spawn_blocking(move || {
            match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(result) => result,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err("等待 WebView2 清理缓存结果超时；操作可能仍在进行或已经完成".to_string())
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err("WebView2 清理缓存回调未执行，操作结果未知".to_string())
                }
            }
        })
        .await
        .map_err(|e| format!("等待 WebView2 清理缓存结果失败: {e}"))?
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("清理浏览器磁盘缓存仅支持 Windows".into())
    }
}
