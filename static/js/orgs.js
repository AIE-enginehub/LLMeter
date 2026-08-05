/**
 * 组织管理 Tab — 组织 CRUD、项目管理、API Key 管理、模型配置、积分充值与流水
 */
function orgsTab() {
  return {
    loading: false,
    orgSubTab: 'info',
    orgs: [],
    selectedOrg: null,
    editOrg: { credit: 0, credit_price: 0 },
    /* 项目相关 */
    projects: [],
    selectedProject: null,
    showCreateProject: false,
    showEditProject: false,
    newProject: { name: '', description: '' },
    editProject: { id: null, name: '', description: '', is_active: true },
    /* Key 相关 */
    keys: [],
    showCreateKey: false,
    createdKey: null,
    newKeyName: '',
    /* 模型配置 */
    models: [],
    showModelForm: false,
    modelForm: { id: null, name: '', protocol: 'openai', model_patterns: '', base_url: '', real_api_key: '', priority: 0, compression_enabled: '' },
    /* 其他 */
    showCreateOrg: false,
    showRecharge: false,
    showCreditLogs: false,
    newOrg: { name: '', slug: '', billing_mode: 'standard_pricing' },
    rechargeAmount: 0,
    rechargeNote: '',
    creditLogs: [],
    creditLogPage: 1,
    creditLogPageSize: 15,
    creditLogTotal: 0,
    creditLogType: '',

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
      this.editOrg = { name: org.name, slug: org.slug, is_active: org.is_active ? 'true' : 'false', billing_mode: org.billing_mode || 'contract_ratio', credit: org.credit || 0, overdraft_limit: org.overdraft_limit || 0, credit_price: org.credit_price || 0, total_consumed: org.total_consumed || 0 };
      this.selectedProject = null;
      this.keys = [];
      await Promise.all([this.loadProjects(), this.loadModels()]);
    },

    // ── 项目管理 ──

    async loadProjects() {
      try {
        this.projects = await api(`/api/orgs/${this.selectedOrg.id}/projects`);
        if (this.projects.length > 0) {
          if (!this.selectedProject) {
            await this.selectProject(this.projects[0]);
          } else {
            const updated = this.projects.find(p => p.id === this.selectedProject.id);
            if (updated) {
              await this.selectProject(updated);
            } else {
              await this.selectProject(this.projects[0]);
            }
          }
        } else {
          this.selectedProject = null;
          this.keys = [];
        }
      } catch { this.projects = []; }
    },

    async selectProject(project) {
      this.selectedProject = project;
      await this.loadKeys();
    },

    async createProjectAction() {
      try {
        const created = await api(`/api/orgs/${this.selectedOrg.id}/projects`, { method: 'POST', body: JSON.stringify(this.newProject) });
        this.newProject = { name: '', description: '' };
        this.showCreateProject = false;
        window.showToast(t('project_created'));
        await this.loadProjects();
        const proj = this.projects.find(p => p.id === created.id);
        if (proj) await this.selectProject(proj);
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    openEditProject(project) {
      this.editProject = { id: project.id, name: project.name, description: project.description, is_active: project.is_active };
      this.showEditProject = true;
    },

    async saveProjectAction() {
      try {
        await api(`/api/projects/${this.editProject.id}`, { method: 'PUT', body: JSON.stringify({
          name: this.editProject.name,
          description: this.editProject.description,
          is_active: this.editProject.is_active
        }) });
        this.showEditProject = false;
        window.showToast(t('save_success'));
        await this.loadProjects();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    async deleteProject(id) {
      if (!await window.showConfirm(t('confirm_delete_project'))) return;
      try {
        await api(`/api/projects/${id}`, { method: 'DELETE' });
        window.showToast(t('project_deleted'));
        if (this.selectedProject && this.selectedProject.id === id) {
          this.selectedProject = null;
          this.keys = [];
        }
        await this.loadProjects();
      } catch (e) { window.showToast(e.message, 'error'); }
    },

    // ── API Key 管理 ──

    async loadKeys() {
      if (!this.selectedProject) { this.keys = []; return; }
      try { this.keys = await api(`/api/projects/${this.selectedProject.id}/keys`); } catch { this.keys = []; }
    },

    async createKey() {
      if (!this.selectedProject) return;
      try {
        const res = await api(`/api/projects/${this.selectedProject.id}/keys`, { method: 'POST', body: JSON.stringify({ name: this.newKeyName }) });
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

    // ── 模型配置 ──

    async loadModels() {
      try { this.models = await api(`/api/orgs/${this.selectedOrg.id}/models`); } catch { this.models = []; }
    },

    openModelForm(model = null) {
      if (model) {
        const ce = model.compression_enabled === true ? 'true' : model.compression_enabled === false ? 'false' : '';
        this.modelForm = { id: model.id, name: model.name, protocol: model.protocol, model_patterns: model.model_patterns, base_url: model.base_url, real_api_key: model.real_api_key, priority: model.priority, compression_enabled: ce };
      } else {
        this.modelForm = { id: null, name: '', protocol: 'openai', model_patterns: '', base_url: '', real_api_key: '', priority: 0, compression_enabled: '' };
      }
      this.showModelForm = true;
    },

    async saveModel() {
      try {
        const ce = this.modelForm.compression_enabled;
        const payload = { ...this.modelForm, compression_enabled: ce === 'true' ? true : ce === 'false' ? false : null };
        if (this.modelForm.id) {
          await api(`/api/models/${this.modelForm.id}`, { method: 'PUT', body: JSON.stringify(payload) });
          window.showToast(t('model_updated'));
        } else {
          await api(`/api/orgs/${this.selectedOrg.id}/models`, { method: 'POST', body: JSON.stringify(payload) });
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

    // ── 组织 CRUD ──

    async createOrg() {
      try {
        const created = await api('/api/orgs', { method: 'POST', body: JSON.stringify(this.newOrg) });
        this.newOrg = { name: '', slug: '', billing_mode: 'standard_pricing' };
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
          billing_mode: this.editOrg.billing_mode,
          overdraft_limit: Number(this.editOrg.overdraft_limit) || 0,
          credit_price: Number(this.editOrg.credit_price) || 0
        }) });
        Object.assign(this.selectedOrg, updated);
        this.editOrg.is_active = updated.is_active;
        this.editOrg.billing_mode = updated.billing_mode;
        this.editOrg.overdraft_limit = updated.overdraft_limit || 0;
        this.editOrg.credit_price = updated.credit_price || 0;
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

    // ── 积分管理 ──

    async rechargeCredit() {
      try {
        const updated = await api(`/api/orgs/${this.selectedOrg.id}/credit`, {
          method: 'POST',
          body: JSON.stringify(this.editOrg.billing_mode === 'standard_pricing'
            ? { amount_yuan: Number(this.rechargeAmount), note: this.rechargeNote }
            : { amount: Number(this.rechargeAmount), note: this.rechargeNote })
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
  };
}
