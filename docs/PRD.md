# gongs-credit 产品需求文档（PRD）

## 一、项目概述

**gongs-credit** 是一个 AI API 中转网关服务，用于统一代理转发各 AI 厂商的 API 请求，并完整记录每次调用的请求/响应内容和 Token 用量。

每个企业 Agent 独立部署一套本服务，统一查看所有调用数据。

## 二、核心功能

### 2.1 API 代理转发

- 支持 OpenAI、Anthropic Claude、Google Gemini、DeepSeek 四个厂商
- 调用方只需修改 `base_url` 为本服务地址，其余参数不变
- 各厂商 API Key 由调用方自行携带，本服务只做透传
- 同时支持流式（SSE）和非流式请求
- 转发过程零侵入，不影响请求速度
- 各厂商的目标地址通过环境变量配置，支持自定义代理地址

### 2.2 数据记录

- 完整记录每次请求的输入（request body）和输出（response body）
- 记录 Token 用量（prompt_tokens、completion_tokens、total_tokens）
- 记录请求耗时、状态码、错误信息
- 流式请求实时记录，不等拼接完成
- 数据存储于 PostgreSQL 数据库

### 2.3 管理后台

- 简单的用户名密码登录认证
- 首次启动自动创建 admin 管理员账号
- 调用日志列表：分页、按厂商/模型/状态筛选
- 用量统计仪表盘：按时间、按模型维度统计
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
    base_url="https://your-domain.com/api/openai/v1",
    api_key="sk-xxx"  # 调用方自己的 Key
)
```

### Anthropic Claude

```python
client = Anthropic(
    base_url="https://your-domain.com/api/anthropic",
    api_key="sk-ant-xxx"
)
```

### Google Gemini

```python
# 将 base_url 指向本服务的 gemini 路径
```

### DeepSeek

```python
client = OpenAI(
    base_url="https://your-domain.com/api/deepseek/v1",
    api_key="sk-xxx"
)
```

## 五、非功能需求

- 转发性能：代理层不引入明显延迟，数据库写入异步执行
- 稳定性：转发失败不影响错误信息回传
- 可部署：支持 Docker Compose 一键部署（后续）
