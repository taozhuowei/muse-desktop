use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Webview};

const HTML_REDIRECT_URL: &str = "https://github.com/taozhuowei/muse-desktop/releases/latest";
const API_URL: &str = "https://api.github.com/repos/taozhuowei/muse-desktop/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

#[derive(Serialize)]
pub struct UpdateCheck {
    available: bool,
    version: String,
}

fn compare_versions(current: &str, tag: &str) -> Result<UpdateCheck, String> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let latest = semver::Version::parse(version).map_err(|e| format!("发布版本无效：{e}"))?;
    let current = semver::Version::parse(current).map_err(|e| format!("当前版本无效：{e}"))?;
    Ok(UpdateCheck {
        available: latest > current,
        version: version.to_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_tags() {
        assert!(compare_versions("0.2.1", "v0.10.0").unwrap().available);
        assert!(!compare_versions("0.2.1", "v0.2.1").unwrap().available);
        assert!(!compare_versions("0.2.1", "0.2.0").unwrap().available);
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
            assert_eq!(tag.unwrap(), "v0.2.0");
        });
    }
}
