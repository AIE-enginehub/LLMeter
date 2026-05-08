# gongs-credit

AI API 中转网关 —— 统一代理转发各 AI 厂商 API 并记录请求/响应/Token 用量。

## 快速开始

```bash
# 安装依赖
pnpm install

# 配置环境变量
cp .env.example .env
# 编辑 .env 填入数据库连接和服务商 base URL

# 初始化数据库
npx prisma migrate dev
npx prisma db seed

# 启动
pnpm dev
```

默认管理员：`admin` / `admin123`

## 添加服务商

在 `.env` 中加一行即可，协议从 URL 路径自动推断（`/v1` → openai, `/anthropic` → anthropic, 其它 → gemini）：

```env
OPENAI_BASE_URL="https://api.openai.com/v1"
DEEPSEEK_BASE_URL="https://api.deepseek.com/v1"
MINIMAX_BASE_URL="https://api.minimaxi.com/anthropic"
GEMINI_BASE_URL="https://generativelanguage.googleapis.com/v1beta"
```

调用方将 SDK 的 base_url 设为 `http://proxy/api/{provider}`：

```python
client = OpenAI(base_url="http://proxy/api/openai", api_key="sk-xxx")
client = OpenAI(base_url="http://proxy/api/deepseek", api_key="sk-xxx")
```

## 文档

- [产品需求](docs/PRD.md)
- [技术方案](docs/TECH_SPEC.md)
