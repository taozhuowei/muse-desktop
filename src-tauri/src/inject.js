(function () {
  'use strict';

  // 避免重复注入
  if (window.__MUSE_NOTIFICATION_INJECTED__) return;
  window.__MUSE_NOTIFICATION_INJECTED__ = true;

  // ==========================================
  // 日志桥接通道：向 Rust 后端与本地日志文件同步
  // ==========================================
  function logToRust(level, message) {
    const text = typeof message === 'object' ? JSON.stringify(message) : String(message);
    try {
      if (window.__TAURI__?.event?.emit) {
        window.__TAURI__.event.emit('muse-log', { level, message: text });
        return;
      }
    } catch (_) {}
    try {
      if (window.__TAURI_INTERNALS__?.invoke) {
        window.__TAURI_INTERNALS__.invoke('log_from_web', { level, message: text });
        return;
      }
    } catch (_) {}
    try {
      if (window.__TAURI_INTERNALS__?.postMessage) {
        window.__TAURI_INTERNALS__.postMessage(JSON.stringify({ cmd: 'log_from_web', level, message: text }));
      }
    } catch (_) {}
  }

  logToRust('INFO', '前端注入脚本 inject.js 初始化成功，当前 URL: ' + window.location.href);

  // 最近发送的通知哈希集合，用于防抖去重（保留 8 秒）
  const recentMap = new Map();
  const DEDUPE_WINDOW_MS = 8000;

  function cleanOldCache() {
    const now = Date.now();
    for (const [key, timestamp] of recentMap.entries()) {
      if (now - timestamp > DEDUPE_WINDOW_MS) {
        recentMap.delete(key);
      }
    }
  }

  // 页面刚加载的前 5 秒为静默初始化期，避免页面初始加载历史消息时造成刷屏
  const STARTUP_TIME = Date.now();
  const STARTUP_SILENCE_MS = 5000;

  function emitNotification(rawTitle, rawBody, source = '未知来源') {
    cleanOldCache();

    let title = (rawTitle || '').toString().trim();
    let body = (rawBody || '').toString().trim();

    if (!title && body) {
      title = 'Muse 提醒';
    } else if (!title && !body) {
      return;
    }

    // 去重键
    const key = `${title}:::${body}`;
    const now = Date.now();
    if (recentMap.has(key) && now - recentMap.get(key) < DEDUPE_WINDOW_MS) {
      logToRust('DEBUG', `[${source}] 命中 8 秒防抖去重，忽略重复通知: ${title}`);
      return;
    }
    recentMap.set(key, now);

    // 如果处于刚打开网页的静默保护期且来源是 DOM 批量加载历史消息，只记录指纹不弹窗
    if (Date.now() - STARTUP_TIME < STARTUP_SILENCE_MS && source === 'DomToast') {
      logToRust('DEBUG', `[${source}] 处于初始加载静默保护期，跳过历史消息弹窗: ${title}`);
      return;
    }

    logToRust('INFO', `[${source}] 成功捕获通知 -> 标题: [${title}], 内容: [${body}]`);

    const payload = { title, body: body || undefined };

    // 双通道尝试发送给 Rust
    let delivered = false;
    try {
      if (window.__TAURI__?.event?.emit) {
        window.__TAURI__.event.emit('muse-notification', payload);
        delivered = true;
      }
    } catch (_) {}

    if (!delivered) {
      try {
        if (window.__TAURI_INTERNALS__?.invoke) {
          window.__TAURI_INTERNALS__.invoke('push_desktop_notification', payload);
          delivered = true;
        }
      } catch (_) {}
    }

    if (!delivered) {
      try {
        if (window.__TAURI_INTERNALS__?.postMessage) {
          window.__TAURI_INTERNALS__.postMessage(JSON.stringify({ cmd: 'push_desktop_notification', ...payload }));
          delivered = true;
        }
      } catch (_) {}
    }

    if (delivered) {
      logToRust('INFO', `[${source}] 通知数据已通过 IPC 递交给 Rust 后端`);
    } else {
      logToRust('WARN', `[${source}] IPC 通道暂不可用，通知发送可能延迟`);
    }
  }

  // ==========================================
  // 1. 劫持 Web 标准 window.Notification
  // ==========================================
  try {
    const OriginalNotification = window.Notification;

    function CustomNotification(title, options = {}) {
      const body = options.body || options.description || '';
      logToRust('INFO', `网页代码调用了 new Notification('${title}', ${JSON.stringify(options)})`);
      emitNotification(title, body, 'WebNotification');

      const dummy = {
        title,
        body,
        icon: options.icon || '',
        tag: options.tag || '',
        data: options.data || null,
        onclick: null,
        onshow: null,
        onerror: null,
        onclose: null,
        close: function () {},
        addEventListener: function (type, listener) {
          if (type === 'show' && typeof listener === 'function') {
            setTimeout(listener, 10);
          }
        },
        removeEventListener: function () {},
        dispatchEvent: function () { return true; }
      };

      return dummy;
    }

    Object.defineProperty(CustomNotification, 'permission', {
      get: () => 'granted',
      set: () => {},
      configurable: true
    });

    CustomNotification.requestPermission = function (cb) {
      logToRust('INFO', '网页请求通知权限 Notification.requestPermission()，自动放行 granted');
      const promise = Promise.resolve('granted');
      if (typeof cb === 'function') cb('granted');
      return promise;
    };

    CustomNotification.maxActions = 2;
    window.Notification = CustomNotification;
    logToRust('INFO', 'window.Notification 劫持与权限放行已就绪');
  } catch (err) {
    logToRust('ERROR', '劫持 window.Notification 失败: ' + err.message);
  }

  // ==========================================
  // 2. 劫持 WebSocket 消息推流
  // ==========================================
  try {
    const OriginalWebSocket = window.WebSocket;
    if (OriginalWebSocket) {
      window.WebSocket = function (...args) {
        const ws = new OriginalWebSocket(...args);
        logToRust('INFO', 'WebSocket 连接建立: ' + (args[0] || ''));

        ws.addEventListener('message', function (event) {
          try {
            if (typeof event.data === 'string') {
              const text = event.data.trim();
              if (text.startsWith('{') && text.endsWith('}')) {
                const data = JSON.parse(text);
                const isNotify =
                  data.type === 'notification' ||
                  data.event === 'notification' ||
                  data.action === 'notify' ||
                  data.channel === 'notifications' ||
                  data.is_notification === true ||
                  (data.status === 'completed' && (data.task || data.video_id || data.title)) ||
                  (data.status === 'ready' && (data.task || data.video_id || data.title));

                if (isNotify) {
                  const title = data.title || data.header || data.event || 'Muse 通知';
                  const body = data.message || data.body || data.text || data.description || (data.status ? `状态：${data.status}` : '');
                  if (title || body) {
                    emitNotification(title, body, 'WebSocket');
                  }
                }
              }
            }
          } catch (_) {}
        });

        return ws;
      };
      window.WebSocket.prototype = OriginalWebSocket.prototype;
      window.WebSocket.CONNECTING = OriginalWebSocket.CONNECTING;
      window.WebSocket.OPEN = OriginalWebSocket.OPEN;
      window.WebSocket.CLOSING = OriginalWebSocket.CLOSING;
      window.WebSocket.CLOSED = OriginalWebSocket.CLOSED;
      logToRust('INFO', 'window.WebSocket 代理已就绪');
    }
  } catch (err) {
    logToRust('ERROR', '代理 window.WebSocket 失败: ' + err.message);
  }

  // ==========================================
  // 3. 劫持 fetch 接口响应 (捕获异步任务状态完成)
  // ==========================================
  try {
    const originalFetch = window.fetch;
    if (originalFetch) {
      window.fetch = async function (...args) {
        const response = await originalFetch.apply(this, args);
        try {
          const url = (typeof args[0] === 'string' ? args[0] : args[0]?.url) || '';
          // 仅关注 API 请求
          if (url.includes('/api/') || url.includes('notification') || url.includes('video') || url.includes('task')) {
            const clone = response.clone();
            clone.json().then(data => {
              if (data && typeof data === 'object') {
                if (data.status === 'completed' || data.status === 'ready' || data.state === 'finished') {
                  const title = data.title || data.name || 'Muse 任务已完成';
                  const body = data.message || data.description || '您的视频或任务已处理完毕';
                  emitNotification(title, body, 'FetchAPI');
                } else if (Array.isArray(data.notifications)) {
                  for (const n of data.notifications) {
                    if (n.unread || n.is_new) {
                      emitNotification(n.title || 'Muse 通知', n.body || n.message || '', 'FetchNotifications');
                    }
                  }
                }
              }
            }).catch(() => {});
          }
        } catch (_) {}
        return response;
      };
      logToRust('INFO', 'window.fetch 接口监听代理已就绪');
    }
  } catch (err) {
    logToRust('ERROR', '代理 window.fetch 失败: ' + err.message);
  }

  // ==========================================
  // 4. 监听 document.title 变化 (后台任务完成后修改标题)
  // ==========================================
  try {
    let lastTitle = document.title;
    const titleObserver = new MutationObserver(() => {
      const currentTitle = document.title;
      if (currentTitle !== lastTitle) {
        const trimmed = currentTitle.trim();
        // 匹配常见带有未读计数或完成标识的标题: "(1)", "[Done]", "Ready", "Completed"
        const countMatch = trimmed.match(/^\((\d+)\)/);
        if (countMatch && parseInt(countMatch[1], 10) > 0) {
          emitNotification('Muse 新提醒', `您有 ${countMatch[1]} 条新消息：${trimmed}`, 'TitleObserver');
        } else if (/ready|complete|finished|完成|就绪/i.test(trimmed)) {
          emitNotification('Muse 任务状态更新', trimmed, 'TitleObserver');
        }
        lastTitle = currentTitle;
      }
    });

    const titleEl = document.querySelector('title');
    if (titleEl) {
      titleObserver.observe(titleEl, { childList: true });
    }
  } catch (err) {
    logToRust('ERROR', '监听 document.title 失败: ' + err.message);
  }

  // ==========================================
  // 5. 监听 DOM 树中的站内弹窗与 Toast (MutationObserver)
  // ==========================================
  function setupDomObserver() {
    if (!document.body) {
      window.addEventListener('DOMContentLoaded', setupDomObserver, { once: true });
      return;
    }

    const toastClassPatterns = /toast|notification|alert|snackbar|notice|banner|message-popup|status-badge|popover/i;

    function inspectNode(node) {
      if (!node || node.nodeType !== 1) return;

      const tag = node.tagName.toLowerCase();
      if (tag === 'script' || tag === 'style' || tag === 'svg' || tag === 'path' || tag === 'audio' || tag === 'video') {
        return;
      }

      const role = node.getAttribute('role');
      const ariaLive = node.getAttribute('aria-live');
      const className = typeof node.className === 'string' ? node.className : '';

      const isToastMatch =
        role === 'alert' ||
        role === 'status' ||
        ariaLive === 'assertive' ||
        ariaLive === 'polite' ||
        toastClassPatterns.test(className) ||
        node.hasAttribute('data-notification') ||
        node.hasAttribute('data-toast');

      if (isToastMatch) {
        const text = (node.innerText || node.textContent || '').trim();
        if (text && text.length >= 2 && text.length <= 300) {
          const titleEl = node.querySelector('h1, h2, h3, h4, strong, .title, [class*="title"], [class*="header"]');
          let title = '';
          let body = text;

          if (titleEl) {
            title = (titleEl.innerText || titleEl.textContent || '').trim();
            body = text.replace(title, '').trim();
          }

          if (!title) {
            title = 'Muse 提醒';
          }

          emitNotification(title, body, 'DomToast');
        }
      }
    }

    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        for (const addedNode of mutation.addedNodes) {
          inspectNode(addedNode);
          if (addedNode.querySelectorAll) {
            const children = addedNode.querySelectorAll('[role="alert"], [role="status"], [class*="toast"], [class*="notification"], [class*="alert"]');
            children.forEach(inspectNode);
          }
        }
      }
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true
    });

    logToRust('INFO', 'DOM 提示与 Toast 观测器 (MutationObserver) 启动完成');
  }

  setupDomObserver();
})();
