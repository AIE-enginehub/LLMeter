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

/** 错误消息国际化映射 */
const ERROR_MAP = {
  '该标识 (slug) 已被其他组织使用': { en: 'This slug is already in use by another organization' },
  '该组织名称已存在': { en: 'Organization name already exists' },
  'API Key 冲突，请重试': { en: 'API Key conflict, please retry' },
  '模型配置名称已存在': { en: 'Model config name already exists' },
  '数据重复，请检查输入': { en: 'Duplicate data, please check your input' },
  '该记录被其他数据引用，无法操作': { en: 'This record is referenced by other data and cannot be modified' },
  '必填字段不能为空': { en: 'Required fields cannot be empty' },
  '操作失败，请稍后重试': { en: 'Operation failed, please try again later' },
  '新密码长度不能少于 6 位': { en: 'Password must be at least 6 characters' },
  '原密码错误': { en: 'Current password is incorrect' },
};

function friendlyError(msg) {
  if (window.lang === 'en' && ERROR_MAP[msg]) return ERROR_MAP[msg].en;
  return msg;
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
    throw new Error(friendlyError(body.error || `Request failed (${res.status})`));
  }
  return res.json();
}
