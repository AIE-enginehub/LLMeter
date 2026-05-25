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

/**
 * 日期范围选择器 Mixin — 提供弹出式日历面板，支持中英文
 * 使用方式：在需要日期筛选的组件中 spread 此 mixin
 */
function dateRangePicker() {
  const pad = n => String(n).padStart(2, '0');
  const today = new Date();
  const fmtDate = d => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

  return {
    _drp_showPanel: '',
    _drp_viewYear: today.getFullYear(),
    _drp_viewMonth: today.getMonth(),

    get _drp_monthLabel() {
      const months_zh = ['1月','2月','3月','4月','5月','6月','7月','8月','9月','10月','11月','12月'];
      const months_en = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
      const m = window.lang === 'en' ? months_en : months_zh;
      return `${this._drp_viewYear} ${m[this._drp_viewMonth]}`;
    },

    get _drp_weekDays() {
      return window.lang === 'en'
        ? ['Mo','Tu','We','Th','Fr','Sa','Su']
        : ['一','二','三','四','五','六','日'];
    },

    get _drp_days() {
      const y = this._drp_viewYear, m = this._drp_viewMonth;
      const firstDay = new Date(y, m, 1).getDay();
      const offset = (firstDay + 6) % 7;
      const daysInMonth = new Date(y, m + 1, 0).getDate();
      const daysInPrev = new Date(y, m, 0).getDate();
      const cells = [];
      for (let i = offset - 1; i >= 0; i--) cells.push({ d: daysInPrev - i, cur: false });
      for (let i = 1; i <= daysInMonth; i++) cells.push({ d: i, cur: true });
      const remaining = 42 - cells.length;
      for (let i = 1; i <= remaining; i++) cells.push({ d: i, cur: false });
      return cells;
    },

    _drp_open(which, currentVal) {
      this._drp_showPanel = which;
      const d = currentVal ? new Date(currentVal) : new Date();
      this._drp_viewYear = d.getFullYear();
      this._drp_viewMonth = d.getMonth();
    },

    _drp_prevMonth() {
      if (this._drp_viewMonth === 0) { this._drp_viewYear--; this._drp_viewMonth = 11; }
      else this._drp_viewMonth--;
    },

    _drp_nextMonth() {
      if (this._drp_viewMonth === 11) { this._drp_viewYear++; this._drp_viewMonth = 0; }
      else this._drp_viewMonth++;
    },

    _drp_select(cell) {
      if (!cell.cur) return;
      const val = fmtDate(new Date(this._drp_viewYear, this._drp_viewMonth, cell.d));
      return val;
    },

    _drp_isToday(cell) {
      return cell.cur && cell.d === today.getDate()
        && this._drp_viewMonth === today.getMonth()
        && this._drp_viewYear === today.getFullYear();
    },

    _drp_fmtDisplay(val) {
      if (!val) return window.lang === 'en' ? 'Select date' : '选择日期';
      return val;
    },
  };
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
