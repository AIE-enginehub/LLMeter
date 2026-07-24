/**
 * 调用日志 Tab — 日志列表、筛选（组织/项目/Key/模型/状态）、分页、详情查看
 */
function logsTab() {
  return {
    ...dateRangePicker(),
    loading: false,
    logs: [],
    orgs: [],
    orgProjects: [],
    orgKeys: [],
    total: 0,
    page: 1,
    pageSize: 20,
    sortOrder: 'desc',
    jumpPage: '',
    filter: { org_id: '', project_id: '', api_key_id: '', model: '', status: '', start_time: '', end_time: '' },
    logDetail: null,
    modelSearchTimer: null,

    selectStartDate(cell) {
      const val = this._drp_select(cell);
      if (val) { this.filter.start_time = val; this._drp_showPanel = ''; this.page = 1; this.load(); }
    },
    selectEndDate(cell) {
      const val = this._drp_select(cell);
      if (val) { this.filter.end_time = val; this._drp_showPanel = ''; this.page = 1; this.load(); }
    },
    clearDateRange() {
      this.filter.start_time = ''; this.filter.end_time = ''; this.page = 1; this.load();
    },

    get totalPages() { return Math.max(1, Math.ceil(this.total / this.pageSize)); },

    toggleTimeSort() {
      this.sortOrder = this.sortOrder === 'desc' ? 'asc' : 'desc';
      this.page = 1;
      this.load();
    },

    goToPage() {
      const requested = Number.parseInt(this.jumpPage, 10);
      if (!Number.isFinite(requested)) return;
      this.page = Math.min(this.totalPages, Math.max(1, requested));
      this.jumpPage = '';
      this.load();
    },

    async load() {
      this.loading = true;
      try {
        if (this.orgs.length === 0) {
          this.orgs = await api('/api/orgs');
        }
        let url = `/api/logs?page=${this.page}&pageSize=${this.pageSize}&sort_order=${this.sortOrder}`;
        if (this.filter.org_id) url += `&org_id=${this.filter.org_id}`;
        if (this.filter.project_id) url += `&project_id=${this.filter.project_id}`;
        if (this.filter.api_key_id) url += `&api_key_id=${this.filter.api_key_id}`;
        if (this.filter.model) url += `&model=${encodeURIComponent(this.filter.model)}`;
        if (this.filter.status) url += `&status=${this.filter.status}`;
        if (this.filter.start_time) url += `&start_time=${encodeURIComponent(this.filter.start_time.replace('T', ' '))}`;
        if (this.filter.end_time) url += `&end_time=${encodeURIComponent(this.filter.end_time.replace('T', ' '))}`;
        if (this.filter.start_time || this.filter.end_time) {
          url += `&timezone_offset=${new Date().getTimezoneOffset()}`;
        }
        const res = await api(url);
        this.logs = res.data;
        this.total = res.total;
      } catch { this.logs = []; }
      this.loading = false;
    },

    /** 切换组织时加载该组织的项目列表 */
    async onOrgChange() {
      this.filter.project_id = '';
      this.filter.api_key_id = '';
      this.orgProjects = [];
      this.orgKeys = [];
      this.page = 1;
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
      this.page = 1;
      if (this.filter.project_id) {
        try {
          this.orgKeys = await api(`/api/projects/${this.filter.project_id}/keys`);
        } catch { this.orgKeys = []; }
      }
      await this.load();
    },

    /** 模型名称输入防抖搜索 (300ms) */
    onModelInput() {
      clearTimeout(this.modelSearchTimer);
      this.modelSearchTimer = setTimeout(() => {
        this.page = 1;
        this.load();
      }, 300);
    },

    async viewLog(id) {
      try {
        this.logDetail = await api(`/api/logs/${id}`);
      } catch (e) { window.showToast(e.message, 'error'); }
    },
  };
}
