"use strict";

const panel = document.getElementById("settings-panel");
const toggle = document.getElementById("settings-toggle");
const closeButton = document.getElementById("close-settings");
const autostart = document.getElementById("autostart");
const cacheButton = document.getElementById("clear-cache");
const updateButton = document.getElementById("check-updates");
const updateDescription = document.getElementById("update-description");
const progressContainer = document.getElementById("update-progress-container");
const progressBar = document.getElementById("update-progress-bar");
const status = document.getElementById("status");
let resizing = false;
let pendingVersion = null;

function invoke(command, args) {
  if (!window.__TAURI__?.core?.invoke) {
    return Promise.reject(new Error("本地服务不可用，请在 Muse Desktop 中打开"));
  }
  return window.__TAURI__.core.invoke(command, args);
}

function message(text, error = false) {
  status.textContent = text;
  status.classList.toggle("error", error);
}

function errorText(error) {
  if (typeof error === "string") return error;
  return error?.message || "未知错误，请重试";
}

async function setExpanded(expanded) {
  if (resizing || expanded === !panel.hidden) return;
  resizing = true;
  toggle.disabled = true;
  closeButton.disabled = true;
  try {
    if (expanded) {
      await invoke("resize_shell_bar", { expanded: true });
      panel.hidden = false;
      toggle.setAttribute("aria-expanded", "true");
      toggle.setAttribute("aria-label", "关闭设置");
      closeButton.focus();
    } else {
      await invoke("resize_shell_bar", { expanded: false });
      panel.hidden = true;
      toggle.setAttribute("aria-expanded", "false");
      toggle.setAttribute("aria-label", "打开设置");
      toggle.focus();
    }
  } catch (error) {
    message(`无法${expanded ? "打开" : "关闭"}设置：${errorText(error)}`, true);
    if (expanded) {
      document.getElementById("version").textContent = "设置不可用";
      toggle.setAttribute("aria-label", `打开设置失败：${errorText(error)}`);
    }
  } finally {
    resizing = false;
    toggle.disabled = false;
    closeButton.disabled = false;
  }
}

toggle.addEventListener("click", () => setExpanded(panel.hidden));
closeButton.addEventListener("click", () => setExpanded(false));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !panel.hidden) {
    event.preventDefault();
    setExpanded(false);
  }
});

autostart.addEventListener("change", async () => {
  const previous = !autostart.checked;
  autostart.disabled = true;
  message("正在保存开机自启设置…");
  try {
    const enabled = await invoke("set_autostart", { enabled: autostart.checked });
    autostart.checked = enabled;
    message(enabled ? "已开启开机自启。" : "已关闭开机自启。");
  } catch (error) {
    autostart.checked = previous;
    message(`保存失败：${errorText(error)}`, true);
  } finally {
    autostart.disabled = false;
  }
});

cacheButton.addEventListener("click", async () => {
  if (!window.confirm("确定清理网页的磁盘缓存吗？登录信息和本地数据会保留。")) return;
  cacheButton.disabled = true;
  message("正在清理缓存…");
  try {
    await invoke("clear_browser_cache");
    message("缓存已清理。");
  } catch (error) {
    message(`清理缓存失败：${errorText(error)}`, true);
  } finally {
    cacheButton.disabled = false;
  }
});

// 监听后端更新下载进度事件
if (window.__TAURI__?.event?.listen) {
  window.__TAURI__.event.listen("update-download-progress", (event) => {
    const payload = event.payload;
    if (!payload) return;

    if (payload.status === "downloading") {
      const pct = payload.percentage || 0;
      const mbDl = (payload.downloaded_bytes / (1024 * 1024)).toFixed(1);
      const mbTotal = (payload.total_bytes / (1024 * 1024)).toFixed(1);
      if (progressContainer) progressContainer.hidden = false;
      if (progressBar) progressBar.style.width = `${pct}%`;
      updateButton.textContent = `下载中 ${pct}%`;
      message(`正在拉取更新：${pct}% (${mbDl} MB / ${mbTotal} MB)`);
    } else if (payload.status === "ready") {
      if (progressBar) progressBar.style.width = "100%";
      updateButton.textContent = "正在安装";
      message("更新包下载完成，正在启动安装程序并重启应用...");
    } else if (payload.status === "error") {
      if (progressContainer) progressContainer.hidden = true;
      updateButton.disabled = false;
      updateButton.textContent = "重试更新";
      message(`更新下载失败：${payload.message || "网络中断"}，请稍后重试。`, true);
    }
  });
}

async function performInstall(version) {
  updateButton.disabled = true;
  updateButton.textContent = "准备拉取...";
  if (progressContainer) progressContainer.hidden = false;
  if (progressBar) progressBar.style.width = "0%";
  message("正在连接更新服务器，准备拉取安装包...");

  try {
    await invoke("download_and_install_update", { version });
  } catch (error) {
    if (progressContainer) progressContainer.hidden = true;
    updateButton.disabled = false;
    updateButton.textContent = "立即更新";
    message(`更新失败：${errorText(error)}`, true);
  }
}

updateButton.addEventListener("click", async () => {
  // 如果已处于待更新状态，直接执行本地拉取更新
  if (pendingVersion) {
    await performInstall(pendingVersion);
    return;
  }

  updateButton.disabled = true;
  updateButton.textContent = "正在检查…";
  if (progressContainer) progressContainer.hidden = true;
  message("正在检查更新…");

  try {
    const result = await invoke("check_updates");
    if (result?.configured === false || result?.status === "not_configured") {
      updateDescription.textContent = "更新服务尚未配置";
      message("更新服务尚未配置，暂时无法检查更新。");
      updateButton.disabled = false;
      updateButton.textContent = "检查更新";
    } else if (result?.available === true) {
      pendingVersion = result.version;
      const v = pendingVersion.startsWith("v") ? pendingVersion : `v${pendingVersion}`;
      updateDescription.textContent = `发现新版本 ${v}，可直接本地更新`;
      message(`发现新版本 ${v}，点击「立即更新」在本地直接拉取安装。`);
      updateButton.textContent = "立即更新";
      updateButton.classList.add("primary");
      updateButton.disabled = false;
    } else if (result?.available === false || result?.status === "up_to_date") {
      pendingVersion = null;
      updateButton.classList.remove("primary");
      updateButton.textContent = "检查更新";
      updateButton.disabled = false;
      const curVer = document.getElementById("version").textContent || "";
      message(`当前已是最新版本 (${curVer})。`);
    } else {
      updateButton.disabled = false;
      updateButton.textContent = "检查更新";
      message("更新检查已完成。");
    }
  } catch (error) {
    updateButton.disabled = false;
    updateButton.textContent = "检查更新";
    const rawMsg = errorText(error);
    if (rawMsg.includes("403") || rawMsg.includes("Rate Limit")) {
      message("GitHub 访问频次受限，请稍后重试。", true);
    } else if (rawMsg.includes("dns") || rawMsg.includes("timeout") || rawMsg.includes("网络连接")) {
      message("网络连接超时，无法连接到 GitHub，请检查网络设置。", true);
    } else {
      message(`检查更新失败：${rawMsg}`, true);
    }
  }
});

async function loadInfo() {
  try {
    const info = await invoke("get_shell_info");
    document.getElementById("version").textContent = info.version ? `v${info.version}` : "版本未知";
    if (typeof info.autostart === "boolean") {
      autostart.checked = info.autostart;
      autostart.disabled = false;
    } else {
      message("无法读取开机自启状态。", true);
    }
    if (info.update_configured === true) {
      updateDescription.textContent = "检查 GitHub Releases 的最新版本";
      updateButton.disabled = false;
    } else {
      updateDescription.textContent = "更新服务尚未配置";
    }
  } catch (error) {
    document.getElementById("version").textContent = "版本不可用";
    message(`无法读取应用信息：${errorText(error)}`, true);
  }
}

loadInfo();
