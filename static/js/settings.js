/**
 * 系统设置 Tab — 积分扣除比例配置 + 修改密码
 */
function settingsTab() {
  return {
    rates: { input_rate: 1221, output_rate: 203.5, cached_rate: 12210, long_context_threshold: null, long_context_multiplier: null },
    pwdForm: { old_password: '', new_password: '', confirm_password: '' },
    showOldPwd: false,
    showNewPwd: false,
    showConfirmPwd: false,

    async load() {
      try {
        this.rates = await api('/api/settings/credit_rates');
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async save() {
      try {
        const payload = { ...this.rates };
        // 阈值和倍率为 0 或空时视为未配置
        if (!payload.long_context_threshold) payload.long_context_threshold = null;
        if (!payload.long_context_multiplier) payload.long_context_multiplier = null;
        await api('/api/settings/credit_rates', { method: 'PUT', body: JSON.stringify(payload) });
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async changePassword() {
      if (this.pwdForm.new_password.length < 6) {
        window.showToast(t('pwd_too_short'), 'error');
        return;
      }
      if (this.pwdForm.new_password !== this.pwdForm.confirm_password) {
        window.showToast(t('pwd_mismatch'), 'error');
        return;
      }
      try {
        await api('/api/auth/password', {
          method: 'PUT',
          body: JSON.stringify({ old_password: this.pwdForm.old_password, new_password: this.pwdForm.new_password })
        });
        this.pwdForm = { old_password: '', new_password: '', confirm_password: '' };
        window.showToast(t('pwd_changed'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },
  };
}
