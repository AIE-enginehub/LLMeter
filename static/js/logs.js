/**
 * 调用日志 Tab — 日志列表、筛选（组织/Key/模型/状态）、分页、详情查看
 */
function logsTab() {
  return {
    loading: false,
    logs: [],
    orgs: [],
    orgKeys: [],
    total: 0,
    page: 1,
    pageSize: 20,
    filter: { org_id: '', api_key_id: '', model: '', status: '' },
    logDetail: null,
    modelSearchTimer: null,

    get totalPages() { return Math.max(1, Math.ceil(this.total / this.pageSize)); },

    async load() {
      this.loading = true;
      try {
        if (this.orgs.length === 0) {
          this.orgs = await api('/api/orgs');
        }
        let url = `/api/logs?page=${this.page}&pageSize=${this.pageSize}`;
        if (this.filter.org_id) url += `&org_id=${this.filter.org_id}`;
        if (this.filter.api_key_id) url += `&api_key_id=${this.filter.api_key_id}`;
        if (this.filter.model) url += `&model=${encodeURIComponent(this.filter.model)}`;
        if (this.filter.status) url += `&status=${this.filter.status}`;
        const res = await api(url);
        this.logs = res.data;
        this.total = res.total;
      } catch { this.logs = []; }
      this.loading = false;
    },

    /** 切换组织时加载该组织的 Key 列表 */
    async onOrgChange() {
      this.filter.api_key_id = '';
      this.orgKeys = [];
      this.page = 1;
      if (this.filter.org_id) {
        try {
          this.orgKeys = await api(`/api/orgs/${this.filter.org_id}/keys`);
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
