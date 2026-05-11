/**
 * 概览 Tab — 统计卡片、按服务商/模型汇总
 */
function overviewTab() {
  return {
    days: 7,
    loading: false,
    stats: null,

    async load() {
      this.loading = true;
      try {
        this.stats = await api(`/api/stats?days=${this.days}`);
      } catch { this.stats = null; }
      this.loading = false;
    },
  };
}
