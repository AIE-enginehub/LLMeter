/**
 * 组织管理 Tab — 组织 CRUD、API Key 管理、模型配置、积分充值与流水
 */
function orgsTab() {
  return {
    loading: false,
    orgs: [],
    selectedOrg: null,
    editOrg: { credit: 0 },
    keys: [],
    models: [],
    showCreateOrg: false,
    showCreateKey: false,
    showModelForm: false,
    showRecharge: false,
    showCreditLogs: false,
    createdKey: null,
    newOrg: { name: '', slug: '' },
    newKeyName: '',
    rechargeAmount: 0,
    rechargeNote: '',
    creditLogs: [],
    creditLogPage: 1,
    creditLogPageSize: 15,
    creditLogTotal: 0,
    creditLogType: '',
    modelForm: { id: null, name: '', protocol: 'openai', model_patterns: '', base_url: '', real_api_key: '', priority: 0 },

    async load() {
      this.loading = true;
      try {
        this.orgs = await api('/api/orgs');
        if (this.orgs.length > 0) {
          if (!this.selectedOrg) {
            await this.selectOrg(this.orgs[0]);
          } else {
            const updatedOrg = this.orgs.find(o => o.id === this.selectedOrg.id);
            if (updatedOrg) {
              await this.selectOrg(updatedOrg);
            } else {
              await this.selectOrg(this.orgs[0]);
            }
          }
        } else {
          this.selectedOrg = null;
        }
      } catch { this.orgs = []; }
      this.loading = false;
    },

    async selectOrg(org) {
      this.selectedOrg = org;
      this.editOrg = { name: org.name, slug: org.slug, is_active: org.is_active ? 'true' : 'false', credit: org.credit || 0, overdraft_limit: org.overdraft_limit || 0, total_consumed: org.total_consumed || 0 };
      await Promise.all([this.loadKeys(), this.loadModels()]);
    },

    async loadKeys() {
      try { this.keys = await api(`/api/orgs/${this.selectedOrg.id}/keys`); } catch { this.keys = []; }
    },

    async loadModels() {
      try { this.models = await api(`/api/orgs/${this.selectedOrg.id}/models`); } catch { this.models = []; }
    },

    async createOrg() {
      try {
        const created = await api('/api/orgs', { method: 'POST', body: JSON.stringify(this.newOrg) });
        this.newOrg = { name: '', slug: '' };
        this.showCreateOrg = false;
        window.showToast(t('org_created'));
        this.orgs = await api('/api/orgs');
        const newOrg = this.orgs.find(o => o.id === created.id);
        if (newOrg) await this.selectOrg(newOrg);
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async saveOrg() {
      try {
        const updated = await api(`/api/orgs/${this.selectedOrg.id}`, { method: 'PUT', body: JSON.stringify({
          name: this.editOrg.name,
          slug: this.editOrg.slug,
          is_active: this.editOrg.is_active === 'true' || this.editOrg.is_active === true,
          overdraft_limit: Number(this.editOrg.overdraft_limit) || 0
        }) });
        Object.assign(this.selectedOrg, updated);
        this.editOrg.is_active = updated.is_active;
        this.editOrg.overdraft_limit = updated.overdraft_limit || 0;
        this.editOrg.total_consumed = updated.total_consumed || 0;
        window.showToast(t('save_success'));
        await this.load();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async deleteOrg() {
      if (!await window.showConfirm(t('confirm_delete_org'))) return;
      try {
        await api(`/api/orgs/${this.selectedOrg.id}`, { method: 'DELETE' });
        this.selectedOrg = null;
        window.showToast(t('org_deleted'));
        await this.load();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async rechargeCredit() {
      try {
        const updated = await api(`/api/orgs/${this.selectedOrg.id}/credit`, {
          method: 'POST',
          body: JSON.stringify({ amount: Number(this.rechargeAmount), note: this.rechargeNote })
        });
        Object.assign(this.selectedOrg, updated);
        this.editOrg.credit = updated.credit;
        this.showRecharge = false;
        this.rechargeAmount = 0;
        this.rechargeNote = '';
        window.showToast(t('recharge_success'));
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async loadCreditLogs(resetPage = true) {
      if (resetPage) this.creditLogPage = 1;
      try {
        const params = new URLSearchParams({ page: this.creditLogPage, page_size: this.creditLogPageSize });
        if (this.creditLogType) params.set('type', this.creditLogType);
        const res = await api(`/api/orgs/${this.selectedOrg.id}/credit_logs?${params}`);
        this.creditLogs = res.data;
        this.creditLogTotal = res.total;
        this.showCreditLogs = true;
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    get creditLogTotalPages() {
      return Math.max(1, Math.ceil(this.creditLogTotal / this.creditLogPageSize));
    },

    async creditLogPrev() {
      if (this.creditLogPage > 1) { this.creditLogPage--; await this.loadCreditLogs(false); }
    },

    async creditLogNext() {
      if (this.creditLogPage < this.creditLogTotalPages) { this.creditLogPage++; await this.loadCreditLogs(false); }
    },

    async createKey() {
      try {
        const res = await api(`/api/orgs/${this.selectedOrg.id}/keys`, { method: 'POST', body: JSON.stringify({ name: this.newKeyName }) });
        this.createdKey = res;
        this.newKeyName = '';
        this.showCreateKey = false;
        window.showToast(t('key_created'));
        await this.loadKeys();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async deleteKey(id) {
      if (!await window.showConfirm(t('confirm_delete_key'))) return;
      try {
        await api(`/api/keys/${id}`, { method: 'DELETE' });
        window.showToast(t('key_deleted'));
        await this.loadKeys();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    openModelForm(model = null) {
      if (model) {
        this.modelForm = { id: model.id, name: model.name, protocol: model.protocol, model_patterns: model.model_patterns, base_url: model.base_url, real_api_key: model.real_api_key, priority: model.priority };
      } else {
        this.modelForm = { id: null, name: '', protocol: 'openai', model_patterns: '', base_url: '', real_api_key: '', priority: 0 };
      }
      this.showModelForm = true;
    },

    async saveModel() {
      try {
        if (this.modelForm.id) {
          await api(`/api/models/${this.modelForm.id}`, { method: 'PUT', body: JSON.stringify(this.modelForm) });
          window.showToast(t('model_updated'));
        } else {
          await api(`/api/orgs/${this.selectedOrg.id}/models`, { method: 'POST', body: JSON.stringify(this.modelForm) });
          window.showToast(t('model_added'));
        }
        this.showModelForm = false;
        await this.loadModels();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async deleteModel(id) {
      if (!await window.showConfirm(t('confirm_delete_model'))) return;
      try {
        await api(`/api/models/${id}`, { method: 'DELETE' });
        window.showToast(t('model_deleted'));
        await this.loadModels();
      } catch (e) { window.showToast(e.message, 'error'); }
    },
  };
}
