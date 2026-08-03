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
    orgProjects: [],
    orgKeys: [],
    exportProjects: [],
    /** 当前视角: 'token' | 'credit' */
    perspective: 'token',
    filter: { org_id: '', project_id: '', api_key_id: '', model: '' },
    exportForm: {
      org_id: '',
      project_ids: [],
      mode: 'month', // month | custom
      month: '',
      start_time: '',
      end_time: '',
      delivery: 'download', // download | email
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
    selectExportStart(cell) {
      const val = this._drp_select(cell);
      if (val) { this.exportForm.start_time = val; this._drp_showPanel = ''; }
    },
    selectExportEnd(cell) {
      const val = this._drp_select(cell);
      if (val) { this.exportForm.end_time = val; this._drp_showPanel = ''; }
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
        if (this.filter.project_id) url += `&project_id=${this.filter.project_id}`;
        if (this.filter.api_key_id) url += `&api_key_id=${this.filter.api_key_id}`;
        if (this.filter.model) url += `&model=${encodeURIComponent(this.filter.model)}`;
        this.stats = await api(url);
      } catch { this.stats = null; }
      this.loading = false;
    },

    /** 切换组织时加载该组织的项目列表 */
    async onOrgChange() {
      this.filter.project_id = '';
      this.filter.api_key_id = '';
      this.orgProjects = [];
      this.orgKeys = [];
      if (this.filter.org_id) {
        try {
          this.orgProjects = await api(`/api/orgs/${this.filter.org_id}/projects`);
        } catch { this.orgProjects = []; }
      }
      await this.load();
    },

    /** 切换项目时加载该项目的 Key 列表 */
    async onProjectChange() {
      this.filter.api_key_id = '';
      this.orgKeys = [];
      if (this.filter.project_id) {
        try {
          this.orgKeys = await api(`/api/projects/${this.filter.project_id}/keys`);
        } catch { this.orgKeys = []; }
      }
      await this.load();
    },

    async onExportOrgChange() {
      this.exportProjects = [];
      this.exportForm.project_ids = [];
      if (!this.exportForm.org_id) return;
      const orgId = this.exportForm.org_id;
      try {
        const projects = await api(`/api/orgs/${orgId}/projects`);
        if (this.exportForm.org_id !== orgId) return;
        this.exportProjects = projects;
        this.exportForm.project_ids = this.exportProjects.map(project => project.id);
      } catch {
        if (this.exportForm.org_id === orgId) this.exportProjects = [];
      }
    },

    toggleExportProject(projectId) {
      const idx = this.exportForm.project_ids.indexOf(projectId);
      if (idx >= 0) this.exportForm.project_ids.splice(idx, 1);
      else this.exportForm.project_ids.push(projectId);
    },

    allExportProjectsSelected() {
      return this.exportProjects.length > 0
        && this.exportForm.project_ids.length === this.exportProjects.length;
    },

    toggleAllExportProjects() {
      this.exportForm.project_ids = this.allExportProjectsSelected()
        ? []
        : this.exportProjects.map(project => project.id);
    },

    async openExport() {
      this.showExportModal = true;
      if (!this.exportForm.recipient_email) this.exportForm.recipient_email = '';
      if (!this.exportForm.org_id && this.filter.org_id) {
        this.exportForm.org_id = this.filter.org_id;
      }
      await this.onExportOrgChange();
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
      if (!this.exportForm.org_id) {
        window.showToast(t('export_org_required'), 'error');
        return;
      }
      if (this.exportProjects.length > 0 && this.exportForm.project_ids.length === 0) {
        window.showToast(t('export_project_required'), 'error');
        return;
      }
      const isEmail = this.exportForm.delivery === 'email';
      if (isEmail && !this.exportForm.recipient_email.trim()) {
        window.showToast(t('export_recipient_required'), 'error');
        return;
      }
      const payload = {
        org_id: this.exportForm.org_id,
        // 全选时发送空数组，由后端解释为该企业的全部项目。
        project_ids: this.allExportProjectsSelected() ? [] : this.exportForm.project_ids,
        month: null,
        start_time: null,
        end_time: null,
        delivery: this.exportForm.delivery,
        recipient_email: isEmail ? this.exportForm.recipient_email.trim() : null,
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
        if (isEmail) {
          await api('/api/usage/export_report', {
            method: 'POST',
            body: JSON.stringify(payload),
          });
          this.showExportModal = false;
          window.showToast(t('export_success'));
        } else {
          const resp = await fetch('/api/usage/export_report', {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(payload),
          });
          if (!resp.ok) {
            const errBody = await resp.json().catch(() => null);
            throw new Error(errBody?.error || resp.statusText);
          }
          const disposition = resp.headers.get('Content-Disposition') || '';
          const match = disposition.match(/filename="?([^"]+)"?/);
          const filename = match ? decodeURIComponent(match[1]) : '流量账单.pdf';
          const blob = await resp.blob();
          const url = URL.createObjectURL(blob);
          const a = document.createElement('a');
          a.href = url; a.download = filename;
          document.body.appendChild(a); a.click();
          document.body.removeChild(a);
          URL.revokeObjectURL(url);
          this.showExportModal = false;
          window.showToast(t('export_download_success'));
        }
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
