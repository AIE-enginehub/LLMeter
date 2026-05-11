/**
 * 用量统计 Tab — 组织筛选、Token/Credit 视角切换、趋势图、模型排行
 */
function usageTab() {
  return {
    days: 7,
    loading: false,
    stats: null,
    orgs: [],
    orgId: '',
    /** 当前视角: 'token' | 'credit' */
    perspective: 'token',

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
        if (this.orgId) url += `&org_id=${this.orgId}`;
        this.stats = await api(url);
      } catch { this.stats = null; }
      this.loading = false;
    },
  };
}
