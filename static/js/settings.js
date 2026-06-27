/**
 * 系统设置 Tab — 积分扣除比例配置 + 修改密码
 */
function settingsTab() {
  return {
    rates: { input_rate: 1221, output_rate: 203.5, cached_rate: 12210, long_context_threshold: null, long_context_input_rate: null, long_context_output_rate: null, long_context_cached_rate: null },
    compression: { enabled: false, mode: 'prose', scope: { system: true, user: true, assistant: false }, min_field_chars: 80, min_savings_pct: 5, max_body_bytes: 8388608, emit_response_header: true },
    pwdForm: { old_password: '', new_password: '', confirm_password: '' },
    showOldPwd: false,
    showNewPwd: false,
    showConfirmPwd: false,

    async load() {
      try {
        this.rates = await api('/api/settings/credit_rates');
      } catch (e) { window.showToast(e.message, 'error'); }
      try {
        this.compression = await api('/api/settings/compression');
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async saveCompression() {
      try {
        await api('/api/settings/compression', { method: 'PUT', body: JSON.stringify(this.compression) });
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async save() {
      try {
        const payload = { ...this.rates };
        // 阈值和比例为 0 或空时视为未配置
        if (!payload.long_context_threshold) payload.long_context_threshold = null;
        if (!payload.long_context_input_rate) payload.long_context_input_rate = null;
        if (!payload.long_context_output_rate) payload.long_context_output_rate = null;
        if (!payload.long_context_cached_rate) payload.long_context_cached_rate = null;
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
