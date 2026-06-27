# LLMeter

[English](README.md) | [中文](README_zh.md)

AI API 代理网关 —— 统一代理转发各 AI 厂商 API，支持多组织管理、API Key 分发、用量统计、请求日志追踪与积分扣费系统。

## 🌟 特性

- **统一代理**：兼容 OpenAI / Gemini / Anthropic 等主流 AI 厂商 API 协议，调用方只需替换 `base_url` 即可无缝接入。
- **多组织管理**：支持为不同团队 / 客户创建独立组织，各组织拥有独立的 API Key 和模型配置。
- **积分系统 (Credit)**：内置积分扣费系统，支持自定义不同类型 Token（输入、输出、缓存）的扣费比例，额度耗尽自动拦截请求。
- **API Key 管理**：安全生成和分发 API Key（`gc-` 前缀），存储 SHA-256 哈希，仅创建时可见完整 Key。
- **模型路由**：支持通配符匹配（如 `gpt-*`、`gemini-*`），按优先级自动路由到对应厂商。
- **用量统计**：实时记录每次请求的 Token 用量（prompt / completion / cached），支持按组织、模型、日期维度聚合。
- **请求日志**：完整记录请求/响应内容，支持分页查询和多条件筛选。
- **管理后台**：内置现代化 Web 管理界面（支持中英文双语），提供组织、Key、模型配置、日志查看、系统设置等功能。
- **流式支持**：完整支持 SSE 流式响应，代理过程中实时转发。
- **提示词压缩**（可选开启）：转发上游前压缩请求体中的自然语言散文，去填充词、改写冗长短语，减少送往外部 LLM 的输入 Token，同时保持 API 契约不变；代码、JSON、工具 Schema 绝不修改。详见 [提示词压缩](#-提示词压缩)。
- **高性能**：基于 Rust (Axum + Tokio) 构建，极低的内存占用与极高的并发处理能力。

## 📸 界面预览

### 概览

![概览](docs/images_zh/首页.png)

### 组织管理

![组织管理](docs/images_zh/组织管理.png)

### 调用日志

![日志列表](docs/images_zh/日志列表.png)

![日志详情](docs/images_zh/日志详情.png)

### 用量统计

![用量统计](docs/images_zh/用量统计.png)

### 系统设置

![系统设置](docs/images_zh/设置.png)

## 🚀 快速开始（Docker Compose）

最简单的启动方式是使用 Docker Compose。

```bash
# 1. 克隆项目
git clone https://github.com/AIE-enginehub/LLMeter.git
cd LLMeter

# 2. 启动服务（包含 PostgreSQL + 应用）
docker compose up -d

# 3. 查看日志
docker compose logs -f app
```

启动后访问 `http://localhost:5000` 进入管理后台。

默认管理员账号：`admin` / 密码：`admin123`（请在生产环境 `.env` 或 `docker-compose.yml` 中修改 `ADMIN_INITIAL_PASSWORD`）

## 🛠️ 本地开发（源码启动）

### 前置要求

- Rust 1.88+
- PostgreSQL 16+

### 步骤

```bash
# 1. 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 配置环境变量
cp .env.example .env
# 编辑 .env，确保 DATABASE_URL 指向你的 PostgreSQL 数据库
# 例如: DATABASE_URL=postgres://postgres:password@localhost:5432/llmeter

# 3. 编译并运行
cargo run

# 或编译 release 版本
cargo build --release
./target/release/llmeter
```

服务启动后会自动执行数据库迁移并创建默认管理员用户。

## ⚙️ 环境变量说明

| 变量名 | 必需 | 默认值 | 说明 |
|---|---|---|---|
| `DATABASE_URL` | 是 | - | PostgreSQL 连接字符串 |
| `AUTH_SECRET` | 是 | - | JWT 签名密钥，生产环境请使用强随机字符串 |
| `ADMIN_INITIAL_PASSWORD` | 否 | `admin123` | 初始管理员密码 |
| `PORT` | 否 | `5000` | 服务监听端口 |

## 🔌 代理调用示例

代理接口兼容原始 AI 厂商 API，只需将 SDK 的 `base_url` 指向本服务即可：

### Python (OpenAI SDK)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:5000/v1",
    api_key="gc-xxxxxxxx"  # 在管理后台创建的 API Key
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
```

### Node.js (OpenAI SDK)

```typescript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:5000/v1",
  apiKey: "gc-xxxxxxxx",
});

const response = await client.chat.completions.create({
  model: "gpt-4o",
  messages: [{ role: "user", content: "Hello!" }],
});
```

Gemini 等其他厂商同理，系统根据请求中的模型名自动匹配路由规则并转发到对应厂商。

## 🗜️ 提示词压缩

LLMeter 可在将请求转发给上游厂商**之前**压缩请求体中的自然语言散文，减少你需要付费的输入
Token。由于积分按上游返回的实际用量扣除，更短的提示词意味着运营方上游账单更低，组织积分消耗
也同步下降——且无需改动任何计费逻辑。

压缩器是一个确定性、保结构的散文过滤器（移植自
[PRECC](https://github.com/peri-a-i/precc-cc)）：去除填充词（`please`、`just`、`basically`
…）、改写冗长短语（`in order to` → `to`）。它是**代码安全**的——围栏代码块、缩进代码、行内
反引号片段、JSON 结构、工具/函数 Schema、图片、`tool`/`assistant` 轮次均不会被修改。默认只压缩
`system` 与 `user` 散文（可配置）。

**默认关闭。** 在管理后台 **系统设置 → 提示词压缩** 中开启并调参，持久化到全局 `compression`
设置：

| 字段 | 含义 | 默认 |
|---|---|---|
| `enabled` | 总开关 | `false` |
| `scope` | 压缩哪些角色（`system`/`user`/`assistant`） | system+user |
| `min_field_chars` | 小于该字符数的字段跳过 | `80` |
| `min_savings_pct` | 节省比例低于该 % 时保留原文 | `5` |
| `max_body_bytes` | 请求体超过该大小时整体跳过 | `8 MiB` |
| `emit_response_header` | 附带 `X-LLMeter-Compression` 信息头 | `true` |

**控制与优先级**（从高到低）：
1. 请求头 `X-LLMeter-Compress: off`（或 `0`/`false`）——本次请求强制透传；`on` 强制开启。该头在
   转发上游前会被剥离。
2. 每模型覆盖——在模型配置上设置 `compression_enabled`（`true`/`false`）覆盖全局开关。
3. 全局 `enabled`。

**透明可验证。** 日志中始终保存原始（未压缩）请求体以供审计；每条请求记录是否压缩及估算节省
Token（在概览卡片、用量统计、日志详情中可见）。如需确认行为一致，可对同一请求发送两次——其中
一次带 `X-LLMeter-Compress: off`——再比对响应。

## 📄 许可证

MIT License
