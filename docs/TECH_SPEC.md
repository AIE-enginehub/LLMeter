# gongs-credit 技术方案

## 一、技术栈

| 组件 | 选型 |
|---|---|
| 框架 | Next.js 15+ (App Router) |
| 语言 | TypeScript |
| ORM | Prisma 6 |
| 数据库 | PostgreSQL 16 |
| UI | Tailwind CSS 4 |
| 认证 | JWT (jose) |
| 包管理 | pnpm |

## 二、架构设计

### 2.1 整体架构

单体 Next.js 全栈应用，包含三个层面：

1. **代理转发层**：统一 Route Handler 接收请求，根据服务商配置透明转发
2. **数据记录层**：异步写入 PostgreSQL，不阻塞转发
3. **管理查看层**：后台页面 + 查询 API

### 2.2 协议与服务商

**三种协议**，解耦协议与服务商：

| 协议 | 说明 |
|---|---|
| openai | 覆盖 chat/completions, responses, embeddings, files, models 等全部端点 |
| anthropic | 覆盖 messages, batches, models, count_tokens 等 |
| gemini | 覆盖 generateContent, streamGenerateContent, embedContent, files, models 等 |

**服务商**通过环境变量动态注册，每个服务商指定 base URL 和使用的协议：

| 服务商 | BASE_URL | 推断协议 | 说明 |
|---|---|---|---|
| openai | `https://api.openai.com/v1` | openai | OpenAI 官方 |
| anthropic | `https://api.anthropic.com/v1` | openai | Anthropic Claude 官方 |
| gemini | `https://generativelanguage.googleapis.com/v1beta` | gemini | Google Gemini 官方 |
| deepseek | `https://api.deepseek.com/v1` | openai | 兼容 OpenAI 协议 |
| minimax | `https://api.minimaxi.com/anthropic` | anthropic | 兼容 Anthropic 协议 |
| *自定义* | 任意 URL | 自动推断 | 只需配一行环境变量 |

### 2.3 路由设计

```
# 代理转发（统一路由，无认证，透传所有路径）
/api/{provider}/[...path]  → 转发到 {PROVIDER}_BASE_URL/{path}

# 示例（调用方 SDK 的 base_url 设为 http://proxy/api/{provider}）：
/api/openai/chat/completions      → https://api.openai.com/v1/chat/completions
/api/deepseek/chat/completions    → https://api.deepseek.com/v1/chat/completions
/api/minimax/v1/messages          → https://api.minimaxi.com/anthropic/v1/messages
/api/gemini/models/gemini-2.5-flash:generateContent → http://host/v1beta/models/gemini-2.5-flash:generateContent

# 管理后台认证
/api/auth/login             → 登录
/api/auth/logout            → 登出
/api/auth/me                → 获取当前用户

# 数据查询（需登录态）
/api/query/logs             → 日志查询（分页/筛选）
/api/query/logs/:id         → 日志详情
/api/query/stats            → 统计查询
/api/query/providers        → 已注册服务商列表

# 页面
/login                      → 登录页
/                           → 数据概览
/logs                       → 日志列表
/stats                      → 统计图表
```

### 2.4 环境变量

```env
DATABASE_URL="postgresql://postgres:root@127.0.0.1:5432/gongs_credit"
AUTH_SECRET="your-jwt-secret"
ADMIN_INITIAL_PASSWORD="admin123"

# 服务商配置：只需 {PROVIDER}_BASE_URL，协议自动推断
# 含 /v1 → openai 协议 | 含 /anthropic → anthropic 协议 | 其它 → gemini 协议
OPENAI_BASE_URL="https://api.openai.com/v1"
ANTHROPIC_BASE_URL="https://api.anthropic.com/v1"
GEMINI_BASE_URL="https://generativelanguage.googleapis.com/v1beta"
DEEPSEEK_BASE_URL="https://api.deepseek.com/v1"
MINIMAX_BASE_URL="https://api.minimaxi.com/anthropic"
```

### 2.5 服务商注册机制

启动时自动扫描所有 `*_BASE_URL` 环境变量，提取服务商名和 base URL，根据 URL 路径自动推断协议。

流程：
1. 遍历 `process.env`，找到所有 `XXX_BASE_URL` 变量
2. 提取服务商名：`OPENAI_BASE_URL` → `openai`
3. 推断协议：URL 路径含 `/anthropic` → anthropic，含 `/v1` → openai，其它 → gemini
4. 构建 `ProviderConfig { name, protocol, baseUrl }`

添加新服务商只需在 `.env` 中加一行，无需修改任何代码。

## 三、数据库设计

### User 表

| 字段 | 类型 | 说明 |
|---|---|---|
| id | String (cuid) | 主键 |
| username | String | 用户名，唯一 |
| passwordHash | String | bcrypt 密码哈希 |
| createdAt | DateTime | 创建时间 |
| updatedAt | DateTime | 更新时间 |

### RequestLog 表

| 字段 | 类型 | 说明 |
|---|---|---|
| id | String (cuid) | 主键 |
| provider | String | 服务商名称（如 openai, deepseek, minimax） |
| model | String? | 模型名称 |
| path | String | 请求路径 |
| method | String | HTTP 方法 |
| isStream | Boolean | 是否流式 |
| requestHeaders | Json? | 请求头（API Key 脱敏） |
| requestBody | Json? | 请求体 |
| responseStatus | Int? | 响应状态码 |
| responseBody | Json? | 响应体 |
| promptTokens | Int? | 输入 Token |
| completionTokens | Int? | 输出 Token |
| cachedTokens | Int? | 缓存命中 Token |
| totalTokens | Int? | 总 Token |
| cost | Decimal? | 费用（预留） |
| status | String | pending/streaming/success/error |
| errorMessage | String? | 错误信息 |
| duration | Int? | 耗时(ms) |
| createdAt | DateTime | 创建时间 |
| completedAt | DateTime? | 完成时间 |

## 四、协议适配器

### Token 用量统一模型

```typescript
interface TokenUsage {
  promptTokens: number;      // 输入 token
  completionTokens: number;  // 输出 token
  cachedTokens: number;      // 缓存命中 token
  totalTokens: number;       // 总计
}
```

### 各协议 Token 映射

| 协议 | 输入 | 输出 | 缓存 |
|---|---|---|---|
| OpenAI (chat) | usage.prompt_tokens | usage.completion_tokens | usage.prompt_tokens_details.cached_tokens |
| OpenAI (responses) | usage.input_tokens | usage.output_tokens | usage.input_tokens_details.cached_tokens |
| Anthropic | usage.input_tokens | usage.output_tokens | usage.cache_read_input_tokens + cache_creation_input_tokens |
| Gemini | usageMetadata.promptTokenCount | usageMetadata.candidatesTokenCount | usageMetadata.cachedContentTokenCount |

### 流式 Token 提取

| 协议 | 流式格式 | Usage 事件 |
|---|---|---|
| OpenAI (chat) | `data: {...}\n\n` + `data: [DONE]` | 最后一个含 usage 的 chunk |
| OpenAI (responses) | `event: type\ndata: {...}\n\n` | `response.completed` 事件 |
| Anthropic | `event: type\ndata: {...}\n\n` | `message_start`(输入) + `message_delta`(输出) |
| Gemini | `data: {...}\n\n` | 最后一个含 usageMetadata 的 chunk |

## 五、转发核心流程

1. 请求进入 `/api/{provider}/...`
2. 查找 `ProviderConfig`（base URL + 协议）
3. 获取对应 `ProtocolAdapter`
4. 读取请求体（支持 JSON/multipart/binary）
5. 提取模型名称 + 判断流式
6. 创建 RequestLog (status: pending)
7. 构建目标 URL：`baseUrl + / + subPath + ?query`
8. 转换请求头 + fetch 转发
9. **非流式**：等待响应 → 提取 usage → 异步更新日志 → 返回响应
10. **流式**：建立 TransformStream → 边转发边提取 usage → 流结束更新日志
11. **异常**：记录错误 → 返回 502

### 性能保障

- 数据库写入完全异步，不阻塞响应
- 使用 ArrayBuffer 转发请求体，支持二进制数据（文件上传等）
- Prisma 客户端全局单例，复用连接池

## 六、目录结构

```
gongs-credit/
├── docs/
│   ├── PRD.md
│   └── TECH_SPEC.md
├── prisma/
│   ├── schema.prisma
│   └── seed.ts
├── src/
│   ├── app/
│   │   ├── api/
│   │   │   ├── [provider]/[...path]/route.ts   # 统一代理转发路由
│   │   │   ├── query/
│   │   │   │   ├── logs/route.ts
│   │   │   │   ├── logs/[id]/route.ts
│   │   │   │   ├── stats/route.ts
│   │   │   │   └── providers/route.ts
│   │   │   └── auth/
│   │   │       ├── login/route.ts
│   │   │       ├── logout/route.ts
│   │   │       └── me/route.ts
│   │   ├── (dashboard)/
│   │   │   ├── layout.tsx
│   │   │   ├── page.tsx
│   │   │   ├── logs/page.tsx
│   │   │   └── stats/page.tsx
│   │   ├── login/page.tsx
│   │   └── layout.tsx
│   ├── lib/
│   │   ├── providers/
│   │   │   ├── base.ts          # 接口定义（ProtocolAdapter, ProviderConfig）
│   │   │   ├── openai.ts        # OpenAI 协议适配器
│   │   │   ├── anthropic.ts     # Anthropic 协议适配器
│   │   │   ├── gemini.ts        # Gemini 协议适配器
│   │   │   └── index.ts         # 服务商注册（环境变量扫描）
│   │   ├── proxy/
│   │   │   └── handler.ts       # 代理转发核心
│   │   ├── auth.ts              # JWT 认证
│   │   └── db.ts                # Prisma 单例
│   └── middleware.ts            # 路由保护
├── .env
├── .env.example
├── package.json
└── next.config.ts
```
