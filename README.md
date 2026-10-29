# Muse Desktop

<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="Muse Desktop Icon" />
</p>

<p align="center">
  <strong>基于 Tauri 2 的超轻量级 Muse AI (muse.ai) 原生桌面客户端</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <img src="https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" />
  <img src="https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Binary%20Size-~6.8MB-brightgreen" alt="Binary Size" />
  <img src="https://img.shields.io/badge/Platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey" alt="Platforms" />
</p>

---

## 📖 项目简介 (Overview)

**Muse Desktop** 是专为 **Muse**（[muse.ai](https://muse.ai)，Meta 旗下的 Personal AI Agent）打造的开源轻量级跨平台桌面客户端。

与传统 Electron 应用动辄消耗上百兆磁盘与数倍内存不同，Muse Desktop 基于 **Tauri 2** 架构构建，直接复用操作系统的原生现代 WebView 内核（Windows Edge WebView2 / macOS WKWebView / Linux WebKitGTK）。在保持原生桌面窗口体验的同时，将二进制体积缩减至惊人的 **6.8 MB**。

---

## ✨ 核心特性 (Features)

- ⚡ **极致轻量**：安装包与独立执行程序仅约 **6.8 MB**，内存占用极低，秒级极速冷启动。
- 🔑 **登录态安全持久化**：依托系统原生 WebView 数据隔离沙箱，在应用内正常登录一次后，Cookie、LocalStorage 与 Token 自动持久化保存，后续打开即用，无需重复登录。
- 🛡️ **纯净无侵入与合规**：
  - 不注入任何自动化外挂脚本，不窃取账号 Cookie，不修改目标网站 DOM。
  - 默认采用系统真实浏览器内核指纹交互，符合 Meta 官方服务协议与账户风控规范。
- 🎨 **原生桌面融合**：
  - 内置官方全套多分辨率高清图标（覆盖窗口顶栏、任务栏图标及快捷方式）。
  - 支持自适应窗口尺寸缩放、记忆窗口位置及原生系统窗口控制。
  - 支持系统托盘（System Tray）常驻与唤醒，点击关闭按钮时可提问最小化到系统托盘或退出程序，支持偏好记忆及托盘菜单重置。
- 🔔 **Windows 原生通知推送**：
  - 全量捕获 muse.ai 站内所有通知（Web 标准 Notification、异步 WebSocket 任务消息、DOM 提示气泡/Toast）。
  - 通过系统原生 Toast 卡片实时推送至 Windows 桌面，最小化托盘后台运行时同样保持秒级监听与推送，不错过视频处理与 AI 分析结果。
- 🌐 **跨平台就绪**：基于标准 Tauri 2 工程架构，代码天然支持 Windows、macOS 和 Linux。

---

## 🖼️ 应用预览 (Preview)

![Muse Desktop Preview](assets/muse-desktop-preview.png)

---

## ⚙️ 它是如何工作的 (Architecture)

```
┌────────────────────────────────────────────────────────┐
│                   Muse Desktop 窗口                     │
│  ┌──────────────────────────────────────────────────┐  │
│  │ 系统原生 WebView (WebView2 / WKWebView / WebKit)  │  │
│  │                                                  │  │
│  │          https://muse.ai 官方在线服务             │  │
│  │                                                  │  │
│  └──────────────────────────────────────────────────┘  │
└───────────────────────────┬────────────────────────────┘
                            │
              持久化隔离沙箱 (App Data Profile)
     `com.tao.muse-desktop` 绑定的独立存储空间
     (保存 Cookies / LocalStorage / Session Token)
```

1. **零前端资源打包**：`tauri.conf.json` 直接加载 `https://muse.ai`，不内嵌繁杂的本地 Web 前端构建产物，完全跟随官方最新功能无缝迭代。
2. **独立的 Session 存储**：所有登录凭证均安全隔离保存在由应用标识符（`com.tao.muse-desktop`）绑定的系统数据目录下，不与系统主浏览器互相干扰。

---

## 🚀 快速开始与安装指南 (Installation Guide)

### 方式一：Windows 标准安装版（强烈推荐 ⭐⭐⭐⭐⭐）

为获得最佳的 Windows 10/11 原生通知体验（**右下角弹窗直接展示 Muse 官方高清图标与应用名称，点击可直接激活桌面窗口**），推荐使用标准安装包：

1. 前往项目的 [Releases](../../releases) 页面，下载最新的安装包文件：  
   `muse-desktop_0.2.0_x64-setup.exe`（仅约 **2.1 MB**）；
2. 双击运行安装程序，按照提示完成快速安装（通常在 2 秒内完成）：
   - 安装器会自动在开始菜单与桌面创建官方图标快捷方式；
   - 自动在 Windows 系统底层注册 `com.tao.muse-desktop` 专属 AUMID，无需手动设置任何注册表；
3. 安装完成后直接启动 Muse，正常登录你的账号；
4. **验证通知功能**：
   - 鼠标右键点击屏幕右下角的 Muse 系统托盘图标；
   - 点击 **「🔔 发送测试通知」**；
   - 桌面右下角将立即弹出标有 Muse 图标的系统级原生 Toast 提醒；
   - 如遇任何问题，可随时点击托盘菜单中的 **「📄 查看运行日志」** 查看端到端运行记录。

> **安装提示**：由于个人开源项目尚未购买昂贵的微软商业数字证书签名，初次运行安装包时 Windows SmartScreen 可能会提示“已保护你的电脑”，请点击 **“更多信息”** -> **“仍要运行”** 即可正常安装。

---

### 方式二：Windows 便携免安装版 (Portable)

如果你不想安装软件，可以直接下载便携版单文件：

1. 前往 [Releases](../../releases) 页面下载 `muse-desktop.exe`；
2. 将其放置在任意目录（如桌面或工具箱文件夹），直接双击运行；
3. 免安装版同样内置全套通知拦截能力及系统 Toast 备用通道，开箱即用。

---

## 📝 更新日志 (Changelog)

### [v0.2.0] - 2026-10-29

#### 🔔 全量支持 muse.ai 网站通知推送到 Windows
- **全方位通知拦截网**：
  - 拦截并模拟 Web 标准 `window.Notification` 构造与权限请求（自动放行 `granted`）；
  - 透明代理底层 `window.WebSocket` 长连接，源头提取异步视频切片、转录与 AI 摘要完成包；
  - 挂载 `MutationObserver` 深度监听页面内动态弹出的 Toast、Alert 及状态气泡；
  - 拦截 `window.fetch` 与接口轮询，自动捕获状态变更为 `ready` / `completed` 的任务。
- **Windows 原生 Toast 通知中心深度集成**：
  - 优先通过 `tauri-plugin-notification` 派发系统通知，正式安装版 100% 展示 Muse 官方高清图标与标题；
  - 配套系统级原生 WinRT 兜底保障机制，确保在各种免安装或便携环境下通知绝不丢失；
  - 加入 8 秒短时特征指纹去重（Deduplication）及长文本截断优化。
- **后台防休眠降频保障**：
  - WebView2 核心参数注入 `--disable-background-timer-throttling`，即使窗口最小化至托盘，后台依然保持秒级实时监听。
- **初始加载 5 秒静默保护期**：
  - 解决初次进入网页批量加载历史已读消息导致弹窗刷屏的问题，历史记录仅收集指纹，仅对启动后的新增消息弹窗。
- **全新本地持久化日志系统与托盘调试菜单**：
  - 自动输出端到端结构化日志至 `%APPDATA%\com.tao.muse-desktop\muse-desktop.log`；
  - 托盘右键菜单新增 **「🔔 发送测试通知」** 与 **「📄 查看运行日志」**，一键自测排查。
- **官方 NSIS 安装包打包发布**：
  - 生成 `muse-desktop_0.2.0_x64-setup.exe`（仅约 2.13 MB），自动化注册系统通知所需 AUMID。

---

### [v0.1.0] - 2026-10-27
- 初始版本发布，提供基于 Tauri 2 的超轻量级 Muse 桌面客户端。
- 支持窗口关闭时选择最小化到系统托盘或退出程序，支持偏好记忆与重置。

### 本地编译构建 (Build from Source)

#### 前置环境要求
1. **Rust 工具链**：请先安装 [Rust 官方工具链](https://rustup.rs/)（Rust 1.77+）。
2. **Node.js**：建议 Node.js 18+ 与 npm。
3. **系统依赖**：
   - **Windows**：需具备 Microsoft Edge WebView2（Windows 10/11 通常已预装）及 C++ 生成工具。
   - **macOS**：需安装 Xcode 命令行工具 (`xcode-select --install`)。
   - **Linux** (Ubuntu/Debian 示例)：
     ```bash
     sudo apt update
     sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
       libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
     ```

#### 开发与打包命令

```bash
# 1. 克隆本仓库
git clone https://github.com/taozhuowei/muse-desktop.git
cd muse-desktop

# 2. 安装前端 CLI 依赖
npm install

# 3. 本地调试运行
npm run dev
# 或直接通过 Cargo 启动:
cd src-tauri && cargo run

# 4. 构建 Release 生产包
npm run build
# 或通过 Cargo 构建高度优化的 Release 可执行程序:
cd src-tauri && cargo build --release
```

编译产物将生成在 `src-tauri/target/release/muse-desktop.exe`。

---

## ❓ 常见问题 (FAQ)

### Q: 为什么首次打开需要登录？支持自动同步本机 Chrome 的登录态吗？
**A:** 本程序出于**账户安全与风控防范**考虑，**绝不主动探测或窃取本机 Chrome 数据库中加密的凭证**。只需在桌面端窗口中正常输入账号密码登录一次，系统 WebView 的持久化沙箱会自动妥善保存会话，之后每次开启都会保持登录状态。

### Q: 离线状态下能使用吗？
**A:** 不能。本程序是 muse.ai 官方云端智能体服务的轻量级原生桌面容器，必须在联网环境下使用。

### Q: 遇到登录风控或人机验证怎么办？
**A:** Meta 对非官方 API 或频繁变动 IP 较为敏感。Muse Desktop 默认使用系统原生无伪造的 WebView2 浏览器指纹，属于合法正常访问。建议在稳定的常规网络或固定节点下使用。

---

## 🔒 免责声明 (Disclaimer)

1. **Muse Desktop 是一个个人开源的第三方非官方桌面客户端**，与 Meta Platforms, Inc. 或 muse.ai 官方团队无任何官方附属、合作、背书或从属关系。
2. “Muse”以及相关品牌 Logo、商标与知识产权归 Meta Platforms, Inc. 及其关联实体所有。
3. 请在遵守 [Muse 服务补充条款](https://muse.ai/terms) 及相关法律法规的前提下使用本项目。

---

## 📄 开源许可证 (License)

本项目采用 [MIT 许可证](LICENSE) 完全开源。任何人均可自由学习、使用、修改与分发。
