# gongs-credit 产品需求文档（PRD）

## 一、项目概述

**gongs-credit** 是一个 AI API 中转网关服务，用于统一代理转发各 AI 厂商的 API 请求，并完整记录每次调用的请求/响应内容和 Token 用量。

每个企业 Agent 独立部署一套本服务，统一查看所有调用数据。

## 二、核心功能

### 2.1 API 代理转发

- 支持三种协议：OpenAI、Anthropic、Gemini
- 支持无限数量的服务商，每个服务商指定协议和目标 base URL
- 添加新服务商只需在 `.env` 中配置环境变量，无需修改代码
- 调用方只需修改 `base_url` 为 `https://your-domain.com/api/{provider}/`，其余参数不变
- 各厂商 API Key 由调用方自行携带，本服务只做透传
- 同时支持流式（SSE）和非流式请求
- 转发过程零侵入，不影响请求速度

### 2.2 数据记录

- 完整记录每次请求的输入（request body）和输出（response body）
- 分别记录 Token 用量：输入（promptTokens）、输出（completionTokens）、缓存（cachedTokens）、总计（totalTokens）
- 记录请求耗时、状态码、错误信息
- 流式请求实时记录，不等拼接完成
- 数据存储于 PostgreSQL 数据库

### 2.3 管理后台

- 简单的用户名密码登录认证
- 首次启动自动创建 admin 管理员账号
- 调用日志列表：分页、按服务商/模型/状态筛选（服务商列表动态加载）
- 用量统计仪表盘：按时间、按服务商维度统计
- 提供 API 接口查询日志和统计数据

### 2.4 计费（预留）

- 数据模型预留 cost 字段
- 后续版本实现具体计费逻辑

## 三、用户角色

| 角色 | 说明 |
|---|---|
| 调用方 Agent | 通过修改 base_url 调用本服务，无需额外认证 |
| 管理员 | 登录后台查看日志、统计数据 |

## 四、调用方使用方式

### OpenAI

```python
client = OpenAI(
    base_url="https://your-domain.com/api/openai",
    api_key="sk-xxx"
)
```

### Anthropic Claude

```python
client = Anthropic(
    base_url="https://your-domain.com/api/anthropic",
    api_key="sk-ant-xxx"
)
```

### DeepSeek（使用 OpenAI 协议）

```python
client = OpenAI(
    base_url="https://your-domain.com/api/deepseek",
    api_key="sk-xxx"
)
```

### MiniMax（使用 Anthropic 协议）

```python
client = Anthropic(
    base_url="https://your-domain.com/api/minimax",
    api_key="your-key"
)
```

## 五、环境变量配置示例

```env
# 只需配置 {PROVIDER}_BASE_URL，协议从 URL 路径自动推断：
#   含 /v1 → openai | 含 /anthropic → anthropic | 其它 → gemini
OPENAI_BASE_URL="https://api.openai.com/v1"
ANTHROPIC_BASE_URL="https://api.anthropic.com/v1"
GEMINI_BASE_URL="https://generativelanguage.googleapis.com/v1beta"
DEEPSEEK_BASE_URL="https://api.deepseek.com/v1"
MINIMAX_BASE_URL="https://api.minimaxi.com/anthropic"
```

## 六、非功能需求

- 转发性能：代理层不引入明显延迟，数据库写入异步执行
- 稳定性：转发失败不影响错误信息回传
- 可扩展：添加新服务商零代码改动
- 可部署：支持 Docker Compose 一键部署（后续）
