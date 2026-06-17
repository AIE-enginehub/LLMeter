/**
 * 用量统计 Tab — 组织/Key/模型筛选、Token/Credit 视角切换、趋势图、模型排行
 */
function usageTab() {
  return {
    ...dateRangePicker(),
    days: 7,
    useCustomRange: false,
    customRange: { start: '', end: '' },
    loading: false,
    exportLoading: false,
    showExportModal: false,
    showExportMonthPanel: false,
    exportMonthYear: new Date().getFullYear(),
    stats: null,
    orgs: [],
    orgKeys: [],
    /** 当前视角: 'token' | 'credit' */
    perspective: 'token',
    filter: { org_id: '', api_key_id: '', model: '' },
    exportForm: {
      org_ids: [],
      mode: 'month', // month | custom
      month: '',
      start_time: '',
      end_time: '',
      recipient_email: '',
    },

    selectCustomStart(cell) {
      const val = this._drp_select(cell);
      if (val) { this.customRange.start = val; this._drp_showPanel = ''; this.load(); }
    },
    selectCustomEnd(cell) {
      const val = this._drp_select(cell);
      if (val) { this.customRange.end = val; this._drp_showPanel = ''; this.load(); }
    },
    modelSearchTimer: null,

    get maxReq() {
      if (!this.stats) return 0;
      return Math.max(...this.stats.daily_stats.map(d => d.request_count), 0);
    },

    get maxToken() {
      if (!this.stats) return 0;
      return Math.max(...this.stats.daily_stats.map(d => d.total_tokens), 0);
    },

    get maxCredit() {
      if (!this.stats) return 0;
      return Math.max(...this.stats.daily_stats.map(d => d.credit_cost || 0), 0);
    },

    async load() {
      this.loading = true;
      try {
        if (this.orgs.length === 0) {
          this.orgs = await api('/api/orgs');
        }
        let url = '/api/stats';
        if (this.useCustomRange && this.customRange.start) {
          url += `?start_time=${encodeURIComponent(this.customRange.start.replace('T', ' '))}`;
          if (this.customRange.end) url += `&end_time=${encodeURIComponent(this.customRange.end.replace('T', ' '))}`;
        } else {
          url += `?days=${this.days}`;
        }
        if (this.filter.org_id) url += `&org_id=${this.filter.org_id}`;
        if (this.filter.api_key_id) url += `&api_key_id=${this.filter.api_key_id}`;
        if (this.filter.model) url += `&model=${encodeURIComponent(this.filter.model)}`;
        this.stats = await api(url);
      } catch { this.stats = null; }
      this.loading = false;
    },

    /** 切换组织时加载该组织的 Key 列表 */
    async onOrgChange() {
      this.filter.api_key_id = '';
      this.orgKeys = [];
      if (this.filter.org_id) {
        try {
          this.orgKeys = await api(`/api/orgs/${this.filter.org_id}/keys`);
        } catch { this.orgKeys = []; }
      }
      await this.load();
    },

    toggleExportOrg(orgId) {
      const idx = this.exportForm.org_ids.indexOf(orgId);
      if (idx >= 0) this.exportForm.org_ids.splice(idx, 1);
      else this.exportForm.org_ids.push(orgId);
    },

    openExport() {
      this.showExportModal = true;
      if (!this.exportForm.recipient_email) this.exportForm.recipient_email = '';
      if (this.exportForm.org_ids.length === 0 && this.filter.org_id) {
        this.exportForm.org_ids = [this.filter.org_id];
      }
      if (!this.exportForm.month) this.exportForm.month = new Date().toISOString().slice(0, 7);
      this.exportMonthYear = Number((this.exportForm.month || '').slice(0, 4)) || new Date().getFullYear();
      this.showExportMonthPanel = false;
    },

    exportMonthDisplay() {
      if (!this.exportForm.month) return t('select_month');
      const [yearStr, monthStr] = this.exportForm.month.split('-');
      const year = Number(yearStr);
      const month = Number(monthStr);
      if (!year || !month) return this.exportForm.month;
      if (window.lang === 'en') {
        const d = new Date(year, month - 1, 1);
        return `${d.toLocaleString('en-US', { month: 'long' })} ${year}`;
      }
      return `${year}年${month}月`;
    },

    exportMonthName(month) {
      if (window.lang === 'en') {
        const d = new Date(2000, month - 1, 1);
        return d.toLocaleString('en-US', { month: 'short' });
      }
      return `${month}月`;
    },

    exportMonthValue(month) {
      return `${this.exportMonthYear}-${String(month).padStart(2, '0')}`;
    },

    isExportMonthSelected(month) {
      return this.exportForm.month === this.exportMonthValue(month);
    },

    pickExportMonth(month) {
      this.exportForm.month = this.exportMonthValue(month);
      this.showExportMonthPanel = false;
    },

    prevExportYear() {
      this.exportMonthYear -= 1;
    },

    nextExportYear() {
      this.exportMonthYear += 1;
    },

    async submitExport() {
      if (this.exportForm.org_ids.length === 0) {
        window.showToast(t('export_org_required'), 'error');
        return;
      }
      if (!this.exportForm.recipient_email.trim()) {
        window.showToast(t('export_recipient_required'), 'error');
        return;
      }
      const payload = {
        org_ids: this.exportForm.org_ids,
        month: null,
        start_time: null,
        end_time: null,
        recipient_email: this.exportForm.recipient_email.trim(),
      };
      if (this.exportForm.mode === 'month') {
        if (!this.exportForm.month) {
          window.showToast(t('export_month_required'), 'error');
          return;
        }
        payload.month = this.exportForm.month;
      } else {
        if (!this.exportForm.start_time || !this.exportForm.end_time) {
          window.showToast(t('export_range_required'), 'error');
          return;
        }
        payload.start_time = this.exportForm.start_time;
        payload.end_time = this.exportForm.end_time;
      }

      this.exportLoading = true;
      try {
        await api('/api/usage/export_report', {
          method: 'POST',
          body: JSON.stringify(payload),
        });
        this.showExportModal = false;
        window.showToast(t('export_success'));
      } catch (e) {
        window.showToast(e.message, 'error');
      } finally {
        this.exportLoading = false;
      }
    },

    /** 模型名称输入防抖搜索 (300ms) */
    onModelInput() {
      clearTimeout(this.modelSearchTimer);
      this.modelSearchTimer = setTimeout(() => this.load(), 300);
    },
  };
}
