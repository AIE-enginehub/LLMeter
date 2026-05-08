# Gongs Credit

AI API 代理网关 —— 统一代理转发各 AI 厂商 API，支持多组织管理、API Key 分发、用量统计与请求日志追踪。

## 特性

- **统一代理**：兼容 OpenAI / Gemini 等主流 AI 厂商 API 协议，调用方只需替换 `base_url` 即可无缝接入
- **多组织管理**：支持为不同团队 / 客户创建独立组织，各组织拥有独立的 API Key 和模型配置
- **API Key 管理**：安全生成和分发 API Key（`gc-` 前缀），存储 SHA-256 哈希，仅创建时可见完整 Key
- **模型路由**：支持通配符匹配（如 `gpt-*`、`gemini-*`），按优先级自动路由到对应厂商
- **用量统计**：实时记录每次请求的 Token 用量（prompt / completion / cached），支持按组织、模型、日期维度聚合
- **请求日志**：完整记录请求/响应内容，支持分页查询和多条件筛选
- **管理后台**：内置 Web 管理界面，提供组织、Key、模型配置、日志查看等功能
- **流式支持**：完整支持 SSE 流式响应，代理过程中实时转发

## 快速开始（Docker Compose）

```bash
# 克隆项目
git clone <your-repo-url>
cd gongs-credit

# 复制并编辑环境变量
cp .env.example .env
# 按需修改 .env 中的配置

# 启动服务（包含 PostgreSQL + 应用）
docker compose up -d

# 查看日志
docker compose logs -f app
```

启动后访问 `http://localhost:3000` 进入管理后台。

默认管理员：`admin` / `admin123`（请在生产环境中修改 `ADMIN_INITIAL_PASSWORD`）

## 本地开发（Rust 环境）

### 前置要求

- Rust 1.87+
- PostgreSQL 16+

### 步骤

```bash
# 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 配置环境变量
cp .env.example .env
# 编辑 .env，确保 DATABASE_URL 指向本地 PostgreSQL

# 编译并运行
cargo run

# 或编译 release 版本
cargo build --release
./target/release/gongs-credit
```

服务启动后会自动执行数据库迁移并创建默认管理员用户。

## 环境变量说明

| 变量名 | 必需 | 默认值 | 说明 |
|---|---|---|---|
| `DATABASE_URL` | 是 | - | PostgreSQL 连接字符串 |
| `AUTH_SECRET` | 是 | - | JWT 签名密钥，生产环境请使用强随机字符串 |
| `ADMIN_INITIAL_PASSWORD` | 否 | `admin123` | 初始管理员密码 |
| `PORT` | 否 | `3000` | 服务监听端口 |
| `ROUTE_{NAME}_MODELS` | 否 | - | 模型匹配模式，逗号分隔，支持 `*` 通配符 |
| `ROUTE_{NAME}_BASE_URL` | 否 | - | 对应厂商的 API 根地址 |

## API 概要

### 认证

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/auth/login` | 管理员登录，返回 JWT Token |
| POST | `/api/auth/logout` | 登出，清除 Cookie |
| GET | `/api/auth/me` | 获取当前登录用户信息 |

### 组织管理

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/orgs` | 列出所有组织 |
| POST | `/api/orgs` | 创建组织 |
| PUT | `/api/orgs/{id}` | 更新组织 |
| DELETE | `/api/orgs/{id}` | 删除组织 |

### API Key 管理

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/orgs/{org_id}/keys` | 列出组织的 API Key |
| POST | `/api/orgs/{org_id}/keys` | 创建 API Key（完整 Key 仅返回一次） |
| DELETE | `/api/keys/{id}` | 禁用 API Key |

### 模型配置

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/orgs/{org_id}/models` | 列出组织的模型配置 |
| POST | `/api/orgs/{org_id}/models` | 创建模型配置 |
| PUT | `/api/models/{id}` | 更新模型配置 |
| DELETE | `/api/models/{id}` | 删除模型配置 |

### 日志与统计

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/logs` | 分页查询请求日志 |
| GET | `/api/logs/{id}` | 日志详情 |
| GET | `/api/stats` | 统计数据（支持按天数、组织筛选） |

### 代理调用

代理接口兼容原始 AI 厂商 API，只需将 SDK 的 `base_url` 指向本服务即可：

```python
from openai import OpenAI

# 原来：https://api.openai.com/v1
# 现在：http://localhost:3000/v1
client = OpenAI(
    base_url="http://localhost:3000/v1",
    api_key="gc-xxxxxxxx"  # 在管理后台创建的 API Key
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
```

```typescript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:3000/v1",
  apiKey: "gc-xxxxxxxx",
});

const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages: [{ role: "user", content: "Hello!" }],
});
```

Gemini 等其他厂商同理，系统根据请求中的模型名自动匹配路由规则并转发到对应厂商。

## 许可证

MIT
