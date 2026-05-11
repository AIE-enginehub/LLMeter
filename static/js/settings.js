/**
 * 系统设置 Tab — 积分扣除比例配置
 */
function settingsTab() {
  return {
    rates: {
      input_rate: 1221,
      output_rate: 203.5,
      cached_rate: 12210
    },

    async load() {
      try {
        this.rates = await api('/api/settings/credit_rates');
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async save() {
      try {
        await api('/api/settings/credit_rates', {
          method: 'PUT',
          body: JSON.stringify(this.rates)
        });
        window.showToast(t('save_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    }
  };
}
