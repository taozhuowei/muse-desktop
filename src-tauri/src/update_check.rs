use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Webview};
use futures_util::StreamExt;
use std::io::Write;

const HTML_REDIRECT_URL: &str = "https://github.com/taozhuowei/muse-desktop/releases/latest";
const API_URL: &str = "https://api.github.com/repos/taozhuowei/muse-desktop/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateCheck {
    pub available: bool,
    pub version: String,
    pub current_version: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateProgressPayload {
    pub status: String,
    pub percentage: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub message: Option<String>,
}

fn compare_versions(current: &str, tag: &str) -> Result<UpdateCheck, String> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let latest = semver::Version::parse(version).map_err(|e| format!("发布版本无效：{e}"))?;
    let current_ver = semver::Version::parse(current).map_err(|e| format!("当前版本无效：{e}"))?;
    Ok(UpdateCheck {
        available: latest > current_ver,
        version: version.to_owned(),
        current_version: current.to_owned(),
    })
}

/// 尝试通过 GitHub 的 HTML 重定向获取最新版本 tag（避免 GitHub API 的 60次/小时 Rate Limit 限流）
async fn fetch_latest_tag_via_redirect(client: &reqwest::Client) -> Result<String, String> {
    let response = client
        .head(HTML_REDIRECT_URL)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = response.status();
    if status.is_redirection() {
        if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
            let loc_str = location.to_str().map_err(|_| "重定向地址包含非法字符".to_string())?;
            // 格式如：https://github.com/taozhuowei/muse-desktop/releases/tag/v0.2.0
            if let Some(idx) = loc_str.rfind("/tag/") {
                let tag = &loc_str[idx + 5..];
                if !tag.is_empty() {
                    return Ok(tag.trim_end_matches('/').to_string());
                }
            }
        }
    }
    
    // 如果 HEAD 没有拿到重定向，尝试 GET（不自动跟随重定向）
    let get_resp = client
        .get(HTML_REDIRECT_URL)
        .send()
        .await
        .map_err(|e| format!("请求重定向失败：{e}"))?;
    
    if get_resp.status().is_redirection() {
        if let Some(location) = get_resp.headers().get(reqwest::header::LOCATION) {
            let loc_str = location.to_str().map_err(|_| "重定向地址包含非法字符".to_string())?;
            if let Some(idx) = loc_str.rfind("/tag/") {
                let tag = &loc_str[idx + 5..];
                if !tag.is_empty() {
                    return Ok(tag.trim_end_matches('/').to_string());
                }
            }
        }
    }

    Err("未从 GitHub 重定向中解析出有效版本号".to_string())
}

/// Fallback：从 GitHub REST API 获取最新版本
async fn fetch_latest_tag_via_api(client: &reqwest::Client) -> Result<String, String> {
    let response = client
        .get(API_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("请求 GitHub API 失败：{e}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        return Err("GitHub 访问频次受限 (Rate Limit Exceeded)，请稍后重试".to_string());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("尚未发布任何正式版本".to_string());
    }
    let checked_resp = response.error_for_status().map_err(|e| format!("GitHub API 响应异常：{e}"))?;
    let body = checked_resp
        .text()
        .await
        .map_err(|e| format!("读取发布信息失败：{e}"))?;
    let release: Release =
        serde_json::from_str(&body).map_err(|e| format!("解析发布信息失败：{e}"))?;
    Ok(release.tag_name)
}

#[tauri::command]
pub async fn check_updates(webview: Webview, app: AppHandle) -> Result<UpdateCheck, String> {
    if webview.label() != "shell" {
        return Err("只有本地设置页面可以检查更新".into());
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("muse-desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("创建更新检查客户端失败：{e}"))?;

    // 优先使用免限流的网页重定向方式
    let tag = match fetch_latest_tag_via_redirect(&client).await {
        Ok(tag) => tag,
        Err(_) => {
            // 重定向失败时尝试 API
            fetch_latest_tag_via_api(&client).await.map_err(|e| {
                format!("检查更新失败：网络连接异常或 {}", e)
            })?
        }
    };

    compare_versions(&app.package_info().version.to_string(), &tag)
}


#[tauri::command]
pub async fn download_and_install_update(
    version: String,
    app: AppHandle,
) -> Result<(), String> {
    crate::log_msg("Updater", "INFO", &format!("开始拉取新版本安装包: v{version}"));

    let client = reqwest::Client::builder()
        .user_agent(concat!("muse-desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

    let filename = format!("muse-desktop_{version}_x64-setup.exe");
    let candidate_urls = [
        format!("https://github.com/taozhuowei/muse-desktop/releases/download/v{version}/{filename}"),
        format!("https://ghfast.top/https://github.com/taozhuowei/muse-desktop/releases/download/v{version}/{filename}"),
        format!("https://ghproxy.net/https://github.com/taozhuowei/muse-desktop/releases/download/v{version}/{filename}"),
    ];

    let temp_dir = std::env::temp_dir().join("muse-desktop-updater");
    let _ = std::fs::create_dir_all(&temp_dir);
    let dest_path = temp_dir.join(&filename);

    let mut response_opt = None;

    for url in &candidate_urls {
        crate::log_msg("Updater", "INFO", &format!("尝试从更新源拉取: {url}"));
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                crate::log_msg("Updater", "INFO", &format!("成功连接更新源: {url}"));
                response_opt = Some(resp);
                break;
            }
            Ok(resp) => {
                crate::log_msg("Updater", "WARN", &format!("更新源响应非 200: {} - {}", url, resp.status()));
            }
            Err(e) => {
                crate::log_msg("Updater", "WARN", &format!("连接更新源失败: {} - {}", url, e));
            }
        }
    }

    let response = response_opt.ok_or_else(|| {
        let err = "所有更新源连接均失败，请检查网络连接".to_string();
        let _ = app.emit(
            "update-download-progress",
            UpdateProgressPayload {
                status: "error".to_string(),
                percentage: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                message: Some(err.clone()),
            },
        );
        err
    })?;

    let total_bytes = response.content_length().unwrap_or(3_340_000);
    let mut downloaded_bytes: u64 = 0;
    let mut stream = response.bytes_stream();

    let mut file = std::fs::File::create(&dest_path).map_err(|e| {
        format!("无法创建临时安装包文件：{e}")
    })?;

    let mut last_emit_percentage = 0;
    let mut last_emit_time = std::time::Instant::now();

    while let Some(chunk_res) = stream.next().await {
        let chunk: bytes::Bytes = chunk_res.map_err(|e| format!("下载数据中断：{e}"))?;
        file.write_all(&chunk).map_err(|e| format!("写入文件失败：{e}"))?;
        downloaded_bytes += chunk.len() as u64;

        let percentage = if total_bytes > 0 {
            ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).min(100.0) as u32
        } else {
            0
        };

        if percentage != last_emit_percentage || last_emit_time.elapsed().as_millis() >= 150 {
            last_emit_percentage = percentage;
            last_emit_time = std::time::Instant::now();
            let _ = app.emit(
                "update-download-progress",
                UpdateProgressPayload {
                    status: "downloading".to_string(),
                    percentage,
                    downloaded_bytes,
                    total_bytes,
                    message: None,
                },
            );
        }
    }

    drop(file);

    crate::log_msg(
        "Updater",
        "INFO",
        &format!("更新包下载完成，文件大小: {downloaded_bytes} 字节，保存路径: {}", dest_path.display()),
    );

    let _ = app.emit(
        "update-download-progress",
        UpdateProgressPayload {
            status: "ready".to_string(),
            percentage: 100,
            downloaded_bytes,
            total_bytes,
            message: Some("下载完成，正在启动安装程序...".to_string()),
        },
    );

    #[cfg(target_os = "windows")]
    {
        crate::log_msg("Updater", "INFO", "启动安装程序并准备退出当前旧应用");
        let spawn_res = std::process::Command::new(&dest_path).arg("/UPDATE").spawn();
        if let Err(e) = spawn_res {
            let err_msg = format!("启动安装程序失败：{e}");
            crate::log_msg("Updater", "ERROR", &err_msg);
            let _ = app.emit(
                "update-download-progress",
                UpdateProgressPayload {
                    status: "error".to_string(),
                    percentage: 100,
                    downloaded_bytes,
                    total_bytes,
                    message: Some(err_msg.clone()),
                },
            );
            return Err(err_msg);
        }

        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            std::process::exit(0);
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_tags() {
        assert!(compare_versions("0.2.4", "v0.10.0").unwrap().available);
        assert!(!compare_versions("0.2.4", "v0.2.4").unwrap().available);
        assert!(!compare_versions("0.2.4", "0.2.0").unwrap().available);
        assert!(!compare_versions("1.0.0", "v1.0.0-rc.1").unwrap().available);
        assert!(compare_versions("1.0.0-rc.1", "v1.0.0").unwrap().available);
        assert!(compare_versions("1.0.0", "not-a-version").is_err());
    }

    #[test]
    fn fetch_redirect_resolves_release_tag() {
        tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();
            let tag = fetch_latest_tag_via_redirect(&client).await;
            println!("Resolved tag: {:?}", tag);
            assert!(tag.is_ok());
            let tag_str = tag.unwrap();
            assert!(tag_str.starts_with('v'));
            assert!(semver::Version::parse(tag_str.trim_start_matches('v')).is_ok());
        });
    }
}
