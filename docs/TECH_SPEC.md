# gongs-credit 技术方案

## 一、技术栈

| 组件 | 选型 |
|---|---|
| 框架 | Next.js 15 (App Router) |
| 语言 | TypeScript |
| ORM | Prisma |
| 数据库 | PostgreSQL 16 |
| UI | Tailwind CSS + shadcn/ui |
| 认证 | JWT (jose) |
| 包管理 | pnpm |

## 二、架构设计

### 2.1 整体架构

单体 Next.js 全栈应用，包含三个层面：

1. **代理转发层**：Route Handlers 接收请求，透明转发到目标厂商
2. **数据记录层**：异步写入 PostgreSQL，不阻塞转发
3. **管理查看层**：后台页面 + 查询 API

### 2.2 路由设计

```
# 代理转发（无认证）
/api/openai/[...path]      → 转发到 OPENAI_BASE_URL
/api/anthropic/[...path]   → 转发到 ANTHROPIC_BASE_URL
/api/gemini/[...path]      → 转发到 GEMINI_BASE_URL
/api/deepseek/[...path]    → 转发到 DEEPSEEK_BASE_URL

# 管理后台认证
/api/auth/login             → 登录
/api/auth/me                → 获取当前用户

# 数据查询（需登录态）
/api/query/logs             → 日志查询
/api/query/stats            → 统计查询

# 页面
/login                      → 登录页
/dashboard                  → 数据概览
/dashboard/logs             → 日志列表
/dashboard/stats            → 统计图表
```

### 2.3 环境变量

```env
DATABASE_URL="postgresql+psycopg://postgres:root@127.0.0.1:5432/gongs_credit"
AUTH_SECRET="your-jwt-secret"
ADMIN_INITIAL_PASSWORD="admin123"

# 各厂商 base URL（支持自定义代理地址）
OPENAI_BASE_URL="https://api.openai.com/v1"
ANTHROPIC_BASE_URL="https://api.anthropic.com"
GEMINI_BASE_URL="https://generativelanguage.googleapis.com/v1beta"
DEEPSEEK_BASE_URL="https://api.deepseek.com/v1"
```

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
| provider | String | 厂商标识 |
| model | String? | 模型名称 |
| path | String | 请求路径 |
| method | String | HTTP 方法 |
| isStream | Boolean | 是否流式 |
| requestHeaders | Json? | 请求头（脱敏） |
| requestBody | Json? | 请求体 |
| responseStatus | Int? | 响应状态码 |
| responseBody | Json? | 响应体 |
| promptTokens | Int? | 输入 Token |
| completionTokens | Int? | 输出 Token |
| totalTokens | Int? | 总 Token |
| cost | Decimal? | 费用（预留） |
| status | String | pending/streaming/success/error |
| errorMessage | String? | 错误信息 |
| duration | Int? | 耗时(ms) |
| createdAt | DateTime | 创建时间 |
| completedAt | DateTime? | 完成时间 |

## 四、Provider 适配器

### 统一接口

```typescript
interface ProviderAdapter {
  name: string;
  buildTargetUrl(path: string, searchParams: URLSearchParams): string;
  transformRequestHeaders(headers: Headers): Headers;
  extractModel(body: any): string | null;
  isStreamRequest(body: any, searchParams: URLSearchParams): boolean;
  extractUsage(responseBody: any): TokenUsage | null;
  extractStreamUsage(chunk: string): TokenUsage | null;
}
```

### 各厂商特点

| 厂商 | 认证方式 | 流式格式 | Usage 位置 |
|---|---|---|---|
| OpenAI | Authorization: Bearer | data: {...}\n\n + [DONE] | response.usage |
| Anthropic | x-api-key + anthropic-version | event: type\ndata: {...} | message_start + message_delta |
| Gemini | query param key= | data: {...}\n\n | usageMetadata |
| DeepSeek | Authorization: Bearer | 同 OpenAI | 同 OpenAI |

## 五、转发核心流程

1. 请求进入 → 创建 RequestLog (status: pending)
2. Provider 适配器处理请求头和 URL
3. fetch 转发到目标厂商
4. 非流式：等待响应 → 记录完整 body 和 usage → 返回
5. 流式：建立 TransformStream 管道 → 边转发边记录 → 提取最终 usage
6. 异常：记录错误 → 返回错误响应

### 性能保障

- 数据库写入不阻塞响应返回
- 流式 chunk 批量更新（减少写入频率）
- Prisma 客户端单例复用连接池

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
│   │   │   ├── openai/[...path]/route.ts
│   │   │   ├── anthropic/[...path]/route.ts
│   │   │   ├── gemini/[...path]/route.ts
│   │   │   ├── deepseek/[...path]/route.ts
│   │   │   ├── query/
│   │   │   │   ├── logs/route.ts
│   │   │   │   └── stats/route.ts
│   │   │   └── auth/
│   │   │       ├── login/route.ts
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
│   │   │   ├── base.ts
│   │   │   ├── openai.ts
│   │   │   ├── anthropic.ts
│   │   │   ├── gemini.ts
│   │   │   └── deepseek.ts
│   │   ├── proxy/
│   │   │   ├── handler.ts
│   │   │   └── stream.ts
│   │   ├── auth.ts
│   │   └── db.ts
│   ├── components/
│   │   └── dashboard/
│   └── middleware.ts
├── tests/
│   └── proxy.test.ts
├── .env
├── .env.example
├── package.json
├── next.config.ts
├── tailwind.config.ts
└── tsconfig.json
```
