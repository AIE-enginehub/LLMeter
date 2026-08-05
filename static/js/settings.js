/**
 * 系统设置 Tab — 子标签切换 + 各分区配置
 */
function settingsTab() {
  return {
    subTab: 'model_prices',

    modelPricings: [],
    editingPricing: null,

    modelRates: [],
    editingRate: null,

    mail: {
      system_contact_email: 'contact@enginehub.cn',
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
        this.modelPricings = await api('/api/settings/model_pricings');
        this.mail = await api('/api/settings/mail');
      } catch (e) { window.showToast(e.message, 'error'); }
      try {
        this.compression = await api('/api/settings/compression');
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    localDateTimeNow() {
      const d = new Date();
      const local = new Date(d.getTime() - d.getTimezoneOffset() * 60000);
      return local.toISOString().slice(0, 16);
    },
    emptyPricing() {
      return {
        pricing_id: null, provider: '', model_name: '', currency: 'CNY', region_type: 'domestic',
        input_price: 0, cached_input_price: null, cache_write_price: null, output_price: 0,
        long_context_threshold: null, long_input_price: null, long_cached_price: null,
        long_cache_write_price: null, long_output_price: null,
        multiplier: 1.3, exchange_rate: 1, effective_at: this.localDateTimeNow(),
      };
    },
    openAddPricing() { this.editingPricing = this.emptyPricing(); },
    openEditPricing(p) {
      const decimalFields = [
        'input_price', 'cached_input_price', 'cache_write_price', 'output_price',
        'long_input_price', 'long_cached_price', 'long_cache_write_price', 'long_output_price',
        'multiplier', 'exchange_rate',
      ];
      this.editingPricing = { ...p, effective_at: this.localDateTimeNow() };
      decimalFields.forEach(field => {
        if (this.editingPricing[field] != null) {
          this.editingPricing[field] = this.fmtPricingNumber(this.editingPricing[field]);
        }
      });
    },
    fmtPricingNumber(value) {
      if (value == null || value === '') return '-';
      return String(value).replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '');
    },
    fmtPricingMoney(value, currency) {
      if (value == null || value === '') return '-';
      const symbol = currency === 'CNY' ? '￥' : '$';
      return symbol + this.fmtPricingNumber(value);
    },
    onPricingCurrencyChange() {
      if (this.editingPricing.currency === 'CNY') {
        this.editingPricing.region_type = 'domestic';
        this.editingPricing.exchange_rate = 1;
        if (Number(this.editingPricing.multiplier) === 1.68) this.editingPricing.multiplier = 1.3;
      } else {
        this.editingPricing.region_type = 'international';
        if (Number(this.editingPricing.exchange_rate) === 1) this.editingPricing.exchange_rate = 7.2;
        if (Number(this.editingPricing.multiplier) === 1.3) this.editingPricing.multiplier = 1.68;
      }
    },
    nullableNumber(value) {
      return value === '' || value == null ? null : Number(value);
    },
    async savePricing() {
      const e = this.editingPricing;
      if (!e.model_name.trim()) {
        window.showToast(t('model_name_required'), 'error'); return;
      }
      const payload = {
        provider: (e.provider || '').trim(), model_name: e.model_name.trim(),
        currency: e.currency, region_type: e.region_type,
        input_price: Number(e.input_price), cached_input_price: this.nullableNumber(e.cached_input_price),
        cache_write_price: this.nullableNumber(e.cache_write_price), output_price: Number(e.output_price),
        long_context_threshold: this.nullableNumber(e.long_context_threshold),
        long_input_price: this.nullableNumber(e.long_input_price), long_cached_price: this.nullableNumber(e.long_cached_price),
        long_cache_write_price: this.nullableNumber(e.long_cache_write_price), long_output_price: this.nullableNumber(e.long_output_price),
        multiplier: Number(e.multiplier), exchange_rate: Number(e.exchange_rate),
        effective_at: new Date(e.effective_at).toISOString(),
      };
      try {
        if (e.pricing_id) {
          await api(`/api/settings/model_pricings/${e.pricing_id}`, { method: 'PUT', body: JSON.stringify(payload) });
        } else {
          await api('/api/settings/model_pricings', { method: 'POST', body: JSON.stringify(payload) });
        }
        this.editingPricing = null;
        this.modelPricings = await api('/api/settings/model_pricings');
        window.showToast(t('save_success'));
      } catch (err) { window.showToast(err.message, 'error'); }
    },
    async deletePricing(id) {
      if (!await window.showConfirm(t('confirm_delete_pricing'))) return;
      try {
        await api(`/api/settings/model_pricings/${id}`, { method: 'DELETE' });
        this.modelPricings = await api('/api/settings/model_pricings');
        window.showToast(t('save_success'));
      } catch (err) { window.showToast(err.message, 'error'); }
    },

    openAddRate() {
      this.editingRate = {
        id: null, model_name: '',
        input_rate: 316, output_rate: 52, cached_rate: null, cache_write_rate: null,
        long_context_threshold: 272000, long_context_input_rate: 158, long_context_output_rate: 35, long_context_cached_rate: null, long_context_cache_write_rate: null,
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
          input_rate: e.input_rate,
          output_rate: e.output_rate,
          cached_rate: e.cached_rate === '' || e.cached_rate == null ? null : Number(e.cached_rate),
          cache_write_rate: e.cache_write_rate === '' || e.cache_write_rate == null ? null : Number(e.cache_write_rate),
          long_context_threshold: e.long_context_threshold || null,
          long_context_input_rate: e.long_context_input_rate || null,
          long_context_output_rate: e.long_context_output_rate || null,
          long_context_cached_rate: e.long_context_cached_rate || null,
          long_context_cache_write_rate: e.long_context_cache_write_rate || null,
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
        const systemContactEmail = (this.mail.system_contact_email || '').trim();
        if (!systemContactEmail) {
          window.showToast(t('mail_system_contact_required'), 'error');
          return;
        }
        const payload = {
          system_contact_email: systemContactEmail,
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
