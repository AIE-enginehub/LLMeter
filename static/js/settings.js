/**
 * 系统设置 Tab — 子标签切换 + 各分区配置
 */
function settingsTab() {
  return {
    subTab: 'model_rates',

    modelRates: [],
    editingRate: null,

    mail: {
      outbound: { host: '', port: 587, username: '', password: '', sender_email: '', sender_name: '', use_tls: true },
    },
    compression: { enabled: false, mode: 'prose', scope: { system: true, user: true, assistant: false }, min_field_chars: 80, min_savings_pct: 5, max_body_bytes: 8388608, emit_response_header: true },
    pwdForm: { old_password: '', new_password: '', confirm_password: '' },
    showOldPwd: false,
    showNewPwd: false,
    showConfirmPwd: false,

    async load() {
      try {
        this.modelRates = await api('/api/settings/model_credit_rates');
        this.mail = await api('/api/settings/mail');
      } catch (e) { window.showToast(e.message, 'error'); }
      try {
        this.compression = await api('/api/settings/compression');
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    openAddRate() {
      this.editingRate = {
        id: null, model_name: '',
        input_rate: 316, output_rate: 52, cached_rate: 3160,
        long_context_threshold: 272000, long_context_input_rate: 158, long_context_output_rate: 35, long_context_cached_rate: 1580,
      };
    },
    openEditRate(r) {
      this.editingRate = { ...r };
    },
    async saveRate() {
      if (!this.editingRate.model_name.trim()) {
        window.showToast(t('model_name_required'), 'error');
        return;
      }
      try {
        const e = this.editingRate;
        const payload = {
          model_name: e.model_name.trim(),
          input_rate: e.input_rate, output_rate: e.output_rate, cached_rate: e.cached_rate,
          long_context_threshold: e.long_context_threshold || null,
          long_context_input_rate: e.long_context_input_rate || null,
          long_context_output_rate: e.long_context_output_rate || null,
          long_context_cached_rate: e.long_context_cached_rate || null,
        };
        if (e.id) {
          await api(`/api/settings/model_credit_rates/${e.id}`, { method: 'PUT', body: JSON.stringify(payload) });
        } else {
          await api('/api/settings/model_credit_rates', { method: 'POST', body: JSON.stringify(payload) });
        }
        this.editingRate = null;
        this.modelRates = await api('/api/settings/model_credit_rates');
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },
    async deleteRate(id) {
      if (!confirm(t('confirm_action'))) return;
      try {
        await api(`/api/settings/model_credit_rates/${id}`, { method: 'DELETE' });
        this.modelRates = await api('/api/settings/model_credit_rates');
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async saveCompression() {
      try {
        await api('/api/settings/compression', { method: 'PUT', body: JSON.stringify(this.compression) });
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async saveMail() {
      try {
        const payload = {
          outbound: {
            host: (this.mail.outbound.host || '').trim(),
            port: Number(this.mail.outbound.port) || 587,
            username: (this.mail.outbound.username || '').trim(),
            password: this.mail.outbound.password || '',
            sender_email: (this.mail.outbound.sender_email || '').trim(),
            sender_name: (this.mail.outbound.sender_name || '').trim(),
            use_tls: !!this.mail.outbound.use_tls,
          }
        };
        await api('/api/settings/mail', { method: 'PUT', body: JSON.stringify(payload) });
        this.mail = payload;
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
