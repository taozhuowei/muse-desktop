(function () {
  'use strict';

  // 避免重复注入
  if (window.__MUSE_NOTIFICATION_INJECTED__) return;
  window.__MUSE_NOTIFICATION_INJECTED__ = true;

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

  function emitNotification(rawTitle, rawBody) {
    cleanOldCache();

    let title = (rawTitle || '').toString().trim();
    let body = (rawBody || '').toString().trim();

    // 如果标题为空但内容不为空，设默认标题
    if (!title && body) {
      title = 'Muse';
    } else if (!title && !body) {
      return;
    }

    // 去重键
    const key = `${title}:::${body}`;
    const now = Date.now();
    if (recentMap.has(key) && now - recentMap.get(key) < DEDUPE_WINDOW_MS) {
      return;
    }
    recentMap.set(key, now);

    const payload = { title, body: body || undefined };

    // 双通道尝试发送给 Rust
    try {
      if (window.__TAURI__?.event?.emit) {
        window.__TAURI__.event.emit('muse-notification', payload);
        return;
      }
    } catch (_) {}

    try {
      if (window.__TAURI_INTERNALS__?.invoke) {
        window.__TAURI_INTERNALS__.invoke('push_desktop_notification', payload);
        return;
      }
    } catch (_) {}

    try {
      if (window.__TAURI_INTERNALS__?.postMessage) {
        window.__TAURI_INTERNALS__.postMessage(JSON.stringify({ cmd: 'push_desktop_notification', ...payload }));
      }
    } catch (_) {}
  }

  // ==========================================
  // 1. 劫持 Web 标准 window.Notification
  // ==========================================
  try {
    const OriginalNotification = window.Notification;

    function CustomNotification(title, options = {}) {
      const body = options.body || options.description || '';
      emitNotification(title, body);

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
      const promise = Promise.resolve('granted');
      if (typeof cb === 'function') cb('granted');
      return promise;
    };

    CustomNotification.maxActions = 2;

    window.Notification = CustomNotification;
  } catch (err) {
    console.error('[Muse Desktop] Hook Notification failed:', err);
  }

  // ==========================================
  // 2. 劫持 WebSocket 消息推流（捕获异步后台通知）
  // ==========================================
  try {
    const OriginalWebSocket = window.WebSocket;
    if (OriginalWebSocket) {
      window.WebSocket = function (...args) {
        const ws = new OriginalWebSocket(...args);

        ws.addEventListener('message', function (event) {
          try {
            if (typeof event.data === 'string') {
              const text = event.data.trim();
              if (text.startsWith('{') && text.endsWith('}')) {
                const data = JSON.parse(text);
                // 常见的通知类事件结构匹配
                const isNotify =
                  data.type === 'notification' ||
                  data.event === 'notification' ||
                  data.action === 'notify' ||
                  data.channel === 'notifications' ||
                  data.is_notification === true ||
                  (data.status === 'completed' && (data.task || data.video_id || data.title));

                if (isNotify) {
                  const title = data.title || data.header || data.event || 'Muse 通知';
                  const body = data.message || data.body || data.text || data.description || (data.status ? `任务状态：${data.status}` : '');
                  if (title || body) {
                    emitNotification(title, body);
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
    }
  } catch (err) {
    console.error('[Muse Desktop] Hook WebSocket failed:', err);
  }

  // ==========================================
  // 3. 监听 DOM 树中的站内弹窗与 Toast (MutationObserver)
  // ==========================================
  function setupDomObserver() {
    if (!document.body) {
      window.addEventListener('DOMContentLoaded', setupDomObserver, { once: true });
      return;
    }

    const toastClassPatterns = /toast|notification|alert|snackbar|notice|banner|message-popup|status-badge/i;

    function inspectNode(node) {
      if (!node || node.nodeType !== 1) return; // 仅检查 Element 节点

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
        // 提取纯文本内容
        const text = (node.innerText || node.textContent || '').trim();
        // 过滤掉超长页面区块或空白
        if (text && text.length >= 2 && text.length <= 300) {
          // 尝试分别提取可能的标题和主体
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

          emitNotification(title, body);
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
  }

  setupDomObserver();
})();
