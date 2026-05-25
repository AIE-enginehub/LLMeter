/**
 * 用量统计 Tab — 组织/Key/模型筛选、Token/Credit 视角切换、趋势图、模型排行
 */
function usageTab() {
  return {
    days: 7,
    loading: false,
    stats: null,
    orgs: [],
    orgKeys: [],
    /** 当前视角: 'token' | 'credit' */
    perspective: 'token',
    filter: { org_id: '', api_key_id: '', model: '' },
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
        let url = `/api/stats?days=${this.days}`;
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

    /** 模型名称输入防抖搜索 (300ms) */
    onModelInput() {
      clearTimeout(this.modelSearchTimer);
      this.modelSearchTimer = setTimeout(() => this.load(), 300);
    },
  };
}
