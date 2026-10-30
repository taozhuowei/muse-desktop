(function () {
  'use strict';

  // 避免重复注入
  if (window.__MUSE_NOTIFICATION_INJECTED__) return;
  window.__MUSE_NOTIFICATION_INJECTED__ = true;

  // ==========================================
  // 1. 应用启动基准与全局防护策略
  // 核心原则：只推送打开应用之后产生的新通知，打开前的历史通知一律不推送
  // ==========================================
  const APP_START_TIME = Date.now();
  // 启动静默基线期（15秒）：页面加载、鉴权、路由跳转、拉取历史未读和渲染历史DOM需要时间，此期间只建基线不发弹窗
  const INITIAL_SYNC_WINDOW_MS = 15000;
  // 突发洪峰判定窗口（1.5秒内超过2条则判定为历史数据/列表批量重绘，自动抑制）
  const BURST_WINDOW_MS = 1500;
  const MAX_BURST_COUNT = 2;
  // 通知去重防抖窗口（8秒）
  const DEDUPE_WINDOW_MS = 8000;

  // 状态基线集合
  const seenBaselineKeys = new Set();
  const seenNotificationIds = new Set();
  const seenTaskIds = new Set();
  const recentDedupeMap = new Map();
  const recentBurstTimestamps = [];
  let isNotificationsInitialized = false;

  // ==========================================
  // 2. 日志桥接通道：向 Rust 后端与本地日志文件同步
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

  logToRust('INFO', `前端注入脚本 inject.js 初始化就绪，当前 URL: ${window.location.href}，启动时间戳: ${APP_START_TIME}`);

  setTimeout(() => {
    logToRust('INFO', '应用初始历史数据同步期已结束（15s），已切换至【实时新通知监听模式】');
  }, INITIAL_SYNC_WINDOW_MS);

  function cleanOldCache() {
    const now = Date.now();
    for (const [key, timestamp] of recentDedupeMap.entries()) {
      if (now - timestamp > DEDUPE_WINDOW_MS) {
        recentDedupeMap.delete(key);
      }
    }
  }

  // 提取通用时间戳并转换为毫秒时间戳
  function extractTimestamp(item) {
    if (!item || typeof item !== 'object') return null;
    const raw = item.created_at || item.createdAt || item.timestamp || item.time || item.date || item.updated_at || item.updatedAt;
    if (!raw) return null;
    if (typeof raw === 'number') {
      return raw < 1e11 ? raw * 1000 : raw;
    }
    const parsed = Date.parse(raw);
    return isNaN(parsed) ? null : parsed;
  }

  // 核心派发网关：严格执行历史过滤与突发抑制
  function emitNotification(rawTitle, rawBody, source = '未知来源', options = {}) {
    cleanOldCache();

    let title = (rawTitle || '').toString().trim();
    let body = (rawBody || '').toString().trim();

    if (!title && body) {
      title = 'Muse 提醒';
    } else if (!title && !body) {
      return;
    }

    const itemKey = `${title}:::${body}`;
    const now = Date.now();

    // 1. 显式时间戳比对：如果消息携带创建/发生时间且早于打开应用时刻，直接丢弃
    if (options.timestamp && typeof options.timestamp === 'number') {
      if (options.timestamp < APP_START_TIME) {
        logToRust('DEBUG', `[${source}] 拦截到早于打开应用的历史通知 (时间: ${new Date(options.timestamp).toLocaleTimeString()})，已丢弃: [${title}]`);
        return;
      }
    }

    // 2. 启动静默基线期（前15秒）：只记录历史基线指纹，不弹窗
    if (now - APP_START_TIME < INITIAL_SYNC_WINDOW_MS && !options.force) {
      seenBaselineKeys.add(itemKey);
      logToRust('DEBUG', `[${source}] 处于启动初始同步期，已收录历史基线，不弹窗: [${title}]`);
      return;
    }

    // 3. 已知历史基线匹配：如果此消息在启动初期已存在，不重复推送
    if (seenBaselineKeys.has(itemKey)) {
      logToRust('DEBUG', `[${source}] 命中启动前已存在的历史基线记录，忽略: [${title}]`);
      return;
    }

    // 4. 突发洪峰抑制保护：短时间内连续涌入多条必为历史列表批量渲染或页面重绘
    recentBurstTimestamps.push(now);
    while (recentBurstTimestamps.length > 0 && now - recentBurstTimestamps[0] > BURST_WINDOW_MS) {
      recentBurstTimestamps.shift();
    }
    if (recentBurstTimestamps.length > MAX_BURST_COUNT) {
      seenBaselineKeys.add(itemKey);
      logToRust('WARN', `[${source}] 1.5秒内连续涌入第 ${recentBurstTimestamps.length} 条通知，判定为历史数据批量加载/页面重绘，已自动抑制: [${title}]`);
      return;
    }

    // 5. 8秒防抖去重
    if (recentDedupeMap.has(itemKey) && now - recentDedupeMap.get(itemKey) < DEDUPE_WINDOW_MS) {
      logToRust('DEBUG', `[${source}] 命中 8 秒防抖去重，忽略重复通知: [${title}]`);
      return;
    }
    recentDedupeMap.set(itemKey, now);
    seenBaselineKeys.add(itemKey);

    logToRust('INFO', `[${source}] 捕获到应用启动后实时新通知 -> 标题: [${title}], 内容: [${body}]`);

    const payload = { title, body: body || undefined };

    // 双通道尝试发送给 Rust 后端
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
  // 3. 劫持 Web 标准 window.Notification
  // ==========================================
  try {
    const OriginalNotification = window.Notification;

    function CustomNotification(title, options = {}) {
      const body = options.body || options.description || '';
      logToRust('INFO', `网页代码调用了 new Notification('${title}', ${JSON.stringify(options)})`);
      emitNotification(title, body, 'WebNotification', {
        timestamp: options.timestamp || Date.now()
      });

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
  // 4. 劫持 WebSocket 实时推流
  // ==========================================
  try {
    const OriginalWebSocket = window.WebSocket;
    if (OriginalWebSocket) {
      window.WebSocket = function (...args) {
        const ws = new OriginalWebSocket(...args);
        const wsUrl = args[0] || '';
        const wsOpenTime = Date.now();
        logToRust('INFO', 'WebSocket 连接建立: ' + wsUrl);

        ws.addEventListener('message', function (event) {
          try {
            if (typeof event.data === 'string') {
              const text = event.data.trim();
              if (text.startsWith('{') && text.endsWith('}')) {
                const data = JSON.parse(text);

                // 提取时间戳
                const itemTime = extractTimestamp(data);
                // 连接建立前 3 秒收到的全量同步消息归入历史基线
                const isConnectionInitialSync = (Date.now() - wsOpenTime) < 3000;

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
                    emitNotification(title, body, 'WebSocket', {
                      timestamp: itemTime,
                      force: !isConnectionInitialSync
                    });
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
  // 5. 劫持 fetch 接口响应 (过滤历史未读列表与历史完成任务)
  // ==========================================
  try {
    const originalFetch = window.fetch;
    if (originalFetch) {
      window.fetch = async function (...args) {
        const response = await originalFetch.apply(this, args);
        try {
          const url = (typeof args[0] === 'string' ? args[0] : args[0]?.url) || '';
          if (url.includes('/api/') || url.includes('notification') || url.includes('video') || url.includes('task')) {
            const clone = response.clone();
            clone.json().then(data => {
              if (!data || typeof data !== 'object') return;

              // 场景 A: 获取通知列表 (data.notifications)
              if (Array.isArray(data.notifications)) {
                if (!isNotificationsInitialized || (Date.now() - APP_START_TIME < INITIAL_SYNC_WINDOW_MS)) {
                  // 首次拉取或启动期内拉取的全为用户离线期间累积的历史通知，全量建立历史基线，不发弹窗
                  for (const n of data.notifications) {
                    const id = n.id || n._id || n.uuid || `${n.title}:${n.created_at || n.message}`;
                    seenNotificationIds.add(id);
                    if (n.title || n.body || n.message) {
                      seenBaselineKeys.add(`${(n.title || 'Muse 通知').trim()}:::${(n.body || n.message || '').trim()}`);
                    }
                  }
                  isNotificationsInitialized = true;
                  logToRust('INFO', `[FetchNotifications] 首次加载发现 ${data.notifications.length} 条通知，已收录为历史基线，不向桌面推送`);
                } else {
                  // 启动后后续增量请求：仅推送未在基线中且生成时间在启动之后的新增通知
                  for (const n of data.notifications) {
                    const id = n.id || n._id || n.uuid || `${n.title}:${n.created_at || n.message}`;
                    if (!seenNotificationIds.has(id)) {
                      seenNotificationIds.add(id);
                      const timestamp = extractTimestamp(n);
                      emitNotification(n.title || 'Muse 通知', n.body || n.message || '', 'FetchNotifications', { timestamp });
                    }
                  }
                }
                return;
              }

              // 场景 B: 异步任务状态响应 (status === 'completed' / 'ready')
              if (data.status === 'completed' || data.status === 'ready' || data.state === 'finished') {
                const taskId = data.id || data.task_id || data.video_id || data.uuid || `${data.title || data.name}`;
                const taskTime = extractTimestamp(data);

                // 如果处于启动期（前15秒），此时查出来的必然是打开应用前就已完成的历史任务，仅建立基线
                if (Date.now() - APP_START_TIME < INITIAL_SYNC_WINDOW_MS) {
                  seenTaskIds.add(taskId);
                  logToRust('DEBUG', `[FetchAPI] 启动期收录已存在的完成任务基线: ${taskId}`);
                  return;
                }

                // 启动稳定后，如果是首次出现且创建/更新时间在启动之后的任务，才进行推送
                if (!seenTaskIds.has(taskId)) {
                  seenTaskIds.add(taskId);
                  const title = data.title || data.name || 'Muse 任务已完成';
                  const body = data.message || data.description || '您的视频或任务已处理完毕';
                  emitNotification(title, body, 'FetchAPI', { timestamp: taskTime });
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
  // 6. 监听 document.title 变化 (仅未读数递增时提醒)
  // ==========================================
  try {
    let lastUnreadCount = 0;
    const initialMatch = (document.title || '').trim().match(/^\((\d+)\)/);
    if (initialMatch) {
      lastUnreadCount = parseInt(initialMatch[1], 10) || 0;
      logToRust('INFO', `初始化网页未读数基线为: ${lastUnreadCount}`);
    }

    const titleObserver = new MutationObserver(() => {
      const currentTitle = (document.title || '').trim();
      const countMatch = currentTitle.match(/^\((\d+)\)/);
      if (countMatch) {
        const currentCount = parseInt(countMatch[1], 10) || 0;
        // 仅在启动静默期后且未读数增长时才提醒
        if (Date.now() - APP_START_TIME >= INITIAL_SYNC_WINDOW_MS && currentCount > lastUnreadCount) {
          const delta = currentCount - lastUnreadCount;
          emitNotification('Muse 新提醒', `您收到了 ${delta} 条新消息`, 'TitleObserver');
        }
        lastUnreadCount = currentCount;
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
  // 7. 监听 DOM 树中的站内独立浮动 Toast (排除历史对话流与常规卡片)
  // ==========================================
  function setupDomObserver() {
    if (!document.body) {
      window.addEventListener('DOMContentLoaded', setupDomObserver, { once: true });
      return;
    }

    // 仅精准匹配专用的浮动 Toast / Snackbar 容器类名
    const strictToastClassPatterns = /\b(toast|toaster|snackbar|notification-toast|alert-toast|toast-popup)\b/i;

    function inspectNode(node) {
      if (!node || node.nodeType !== 1) return;

      const tag = node.tagName.toLowerCase();
      if (tag === 'script' || tag === 'style' || tag === 'svg' || tag === 'path' || tag === 'audio' || tag === 'video') {
        return;
      }

      // 关键排除：绝对不能从普通聊天流、历史会话、日志查看器或常规卡片列表中抓取通知
      if (node.closest('main, article, [role="feed"], [role="log"], [role="list"], .chat, .message, .timeline, .history, .feed, [class*="chat"], [class*="message"], [class*="timeline"], [class*="history"], [class*="conversation"]')) {
        return;
      }

      const role = node.getAttribute('role');
      const ariaLive = node.getAttribute('aria-live');
      const className = typeof node.className === 'string' ? node.className : '';

      // 仅当属于专用浮动 Toast，或显式标记为 data-toast/data-notification 时才处理
      const isToastMatch =
        node.hasAttribute('data-toast') ||
        node.hasAttribute('data-notification') ||
        strictToastClassPatterns.test(className) ||
        ((role === 'alert' || ariaLive === 'assertive') && (node.closest('[class*="toast"], [class*="snackbar"], [class*="toaster"]') !== null));

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
            const children = addedNode.querySelectorAll('[data-toast], [data-notification], [class*="toast"], [class*="snackbar"]');
            children.forEach(inspectNode);
          }
        }
      }
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true
    });

    logToRust('INFO', 'DOM 独立浮动 Toast 观测器 (MutationObserver) 启动完成');
  }

  setupDomObserver();
})();
