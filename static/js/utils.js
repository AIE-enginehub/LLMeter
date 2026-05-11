/**
 * 通用工具函数 — 格式化、API 请求、剪贴板、Toast、确认弹窗
 */

/** Token 格式化：>= 1M 显示 "1.2M"，>= 1K 显示 "1.2K" */
function fmtToken(n) {
  if (n == null) return '-';
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M';
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
  return String(n);
}

/** 数值格式化（带千分位，按当前语言选择 locale） */
function formatNum(n) {
  if (n == null) return '-';
  const locale = window.lang === 'en' ? 'en-US' : 'zh-CN';
  return Number(n).toLocaleString(locale);
}

/** 时间格式化（按当前语言选择 locale） */
function fmtTime(t) {
  if (!t) return '-';
  const locale = window.lang === 'en' ? 'en-US' : 'zh-CN';
  return new Date(t).toLocaleString(locale);
}

/**
 * 显示 Toast 提示（通过 Alpine.js app 组件的响应式数据驱动）
 * @param {string} message - 提示文本
 * @param {'success'|'error'|'info'|'warning'} type - 类型
 */
window.showToast = function(message, type = 'success') {
  const appEl = document.querySelector('[x-data="app()"]');
  const data = appEl && appEl._x_dataStack && appEl._x_dataStack[0];
  if (data && data.toasts) {
    const id = Date.now();
    data.toasts.push({ id, message, type, show: true });
    setTimeout(() => {
      const toast = data.toasts.find(t => t.id === id);
      if (toast) toast.show = false;
      setTimeout(() => {
        data.toasts = data.toasts.filter(t => t.id !== id);
      }, 300);
    }, 3000);
  } else {
    console.warn('[showToast fallback]', message);
  }
};

/**
 * 显示自定义确认弹窗，返回 Promise<boolean>
 * @param {string} message - 确认提示文本
 */
window.showConfirm = function(message) {
  return new Promise((resolve) => {
    const appEl = document.querySelector('[x-data="app()"]');
    const data = appEl && appEl._x_dataStack && appEl._x_dataStack[0];
    if (data) {
      data.confirmDialog = { show: true, message, resolve };
    } else {
      resolve(false);
    }
  });
};

/** 复制文本到剪贴板（兼容非 HTTPS 环境） */
async function copyText(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
    } else {
      let textArea = document.createElement("textarea");
      textArea.value = text;
      textArea.style.position = "fixed";
      textArea.style.left = "-999999px";
      textArea.style.top = "-999999px";
      document.body.appendChild(textArea);
      textArea.focus();
      textArea.select();
      document.execCommand('copy');
      textArea.remove();
    }
    window.showToast(window.t('copy_success'));
  } catch (err) {
    window.showToast(window.t('copy_fail'), "error");
  }
}

/** 封装 fetch 请求，自动处理 JSON 和错误 */
async function api(url, options = {}) {
  const res = await fetch(url, {
    credentials: 'same-origin',
    ...options,
    headers: { 'Content-Type': 'application/json', ...options.headers }
  });
  if (res.status === 204) return null;
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `Request failed (${res.status})`);
  }
  return res.json();
}
