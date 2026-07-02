/**
 * 全局应用状态 — 登录 / 登出 / Tab 切换 / Toast / 确认弹窗
 */
function app() {
  return {
    t: window.t,
    user: null,
    tab: ['overview','orgs','logs','usage','settings'].includes(location.hash.slice(1)) ? location.hash.slice(1) : 'overview',
    tabs: [
      { key: 'overview', label: 'overview' },
      { key: 'orgs', label: 'orgs' },
      { key: 'logs', label: 'logs' },
      { key: 'usage', label: 'usage' },
      { key: 'settings', label: 'settings' },
    ],

    loginForm: { username: '', password: '' },
    loginLoading: false,
    loginError: '',
    toasts: [],
    confirmDialog: { show: false, message: '', resolve: null },

    async init() {
      try {
        this.user = await api('/api/auth/me');
      } catch {
        this.user = null;
      }
    },

    async login() {
      this.loginLoading = true;
      this.loginError = '';
      try {
        const res = await api('/api/auth/login', { method: 'POST', body: JSON.stringify(this.loginForm) });
        this.user = res.user;
        window.showToast(t('login_success'));
      } catch (e) {
        this.loginError = e.message;
        window.showToast(e.message, 'error');
      } finally {
        this.loginLoading = false;
      }
    },

    async logout() {
      try {
        await api('/api/auth/logout', { method: 'POST' });
        window.showToast(t('logout_success'));
      } catch {}
      this.user = null;
    },
  };
}
