# Open API 对接文档

LLMeter 提供开放 API 供各组织查询自身的 Credit（积分）消耗情况，方便与内部系统对接。

## 鉴权

所有接口使用组织的 **API Key** 进行鉴权（即代理转发使用的同一 Key），在请求 Header 中以 Bearer Token 方式传入：

```
Authorization: Bearer gc-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

系统根据 API Key 自动识别所属组织，无需额外传入组织标识。

> **注意**：API Key 仅在创建时显示一次，请妥善保管。

---

## 错误响应格式

所有接口的错误响应遵循统一格式：

```json
{
  "error": "错误描述信息"
}
```

常见 HTTP 状态码：

| 状态码 | 说明 |
|--------|------|
| 200 | 成功 |
| 401 | 未提供 API Key 或 Key 无效/已停用 |
| 500 | 服务器内部错误 |

---

## 接口列表

### 1. 查询 Credit 余额

查询当前组织的 Credit 余额。

**请求**

```
GET /open-api/credit/balance
```

**响应**

```json
{
  "org_name": "Gongs",
  "slug": "gongs",
  "credit_balance": "125.3400"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `org_name` | string | 组织名称 |
| `slug` | string | 组织标识 |
| `credit_balance` | string | 当前余额（保留 4 位小数） |

**curl 示例**

```bash
curl -H "Authorization: Bearer gc-xxxxxxxxxxxxxxxx" \
  http://localhost:5000/open-api/credit/balance
```

---

### 2. 查询 Credit 消耗统计

查询指定时间区间内的 Credit 消耗汇总及每日明细。

**请求**

```
GET /open-api/credit/usage?start={start}&end={end}
```

**查询参数**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `start` | string (ISO 8601) | 否 | 开始时间，默认 30 天前。示例：`2026-05-01T00:00:00Z` |
| `end` | string (ISO 8601) | 否 | 结束时间，默认当前时间。示例：`2026-05-11T23:59:59Z` |

**响应**

```json
{
  "start": "2026-05-01T00:00:00Z",
  "end": "2026-05-11T23:59:59Z",
  "total_credit_cost": "12.5678",
  "total_requests": 156,
  "daily": [
    {
      "date": "2026-05-08",
      "credit_cost": 5.1234,
      "request_count": 80
    },
    {
      "date": "2026-05-11",
      "credit_cost": 7.4444,
      "request_count": 76
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `start` | string | 实际查询的开始时间 |
| `end` | string | 实际查询的结束时间 |
| `total_credit_cost` | string | 区间内总 Credit 消耗 |
| `total_requests` | number | 区间内总请求数 |
| `daily` | array | 每日消耗明细 |
| `daily[].date` | string | 日期 (YYYY-MM-DD) |
| `daily[].credit_cost` | number | 当日 Credit 消耗 |
| `daily[].request_count` | number | 当日请求数 |

**curl 示例**

```bash
curl -H "Authorization: Bearer gc-xxxxxxxxxxxxxxxx" \
  "http://localhost:5000/open-api/credit/usage?start=2026-05-01T00:00:00Z&end=2026-05-11T23:59:59Z"
```

---

### 3. 查询 Credit 流水

查询指定时间区间内的 Credit 变动流水（充值/消耗），支持分页。

**请求**

```
GET /open-api/credit/logs?start={start}&end={end}&page={page}&page_size={page_size}
```

**查询参数**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `start` | string (ISO 8601) | 否 | 开始时间，默认 30 天前 |
| `end` | string (ISO 8601) | 否 | 结束时间，默认当前时间 |
| `page` | number | 否 | 页码，默认 `1` |
| `page_size` | number | 否 | 每页条数，默认 `50`，最大 `100` |

**响应**

```json
{
  "data": [
    {
      "id": "a1b2c3d4-...",
      "amount": "-1.3178",
      "balance_after": "98.6822",
      "transaction_type": "consume",
      "note": "消耗",
      "created_at": "2026-05-11T07:00:23Z"
    },
    {
      "id": "e5f6g7h8-...",
      "amount": "100.0000",
      "balance_after": "100.0000",
      "transaction_type": "recharge",
      "note": "初始充值",
      "created_at": "2026-05-01T10:00:00Z"
    }
  ],
  "total": 2,
  "page": 1,
  "page_size": 50
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `data` | array | 流水记录列表 |
| `data[].id` | string (UUID) | 流水记录 ID |
| `data[].amount` | string | 变动金额。正数为充值，负数为消耗 |
| `data[].balance_after` | string | 变动后余额 |
| `data[].transaction_type` | string | 类型：`recharge`（充值）或 `consume`（消耗） |
| `data[].note` | string \| null | 备注信息 |
| `data[].created_at` | string | 发生时间 (ISO 8601) |
| `total` | number | 符合条件的总记录数 |
| `page` | number | 当前页码 |
| `page_size` | number | 每页条数 |

**curl 示例**

```bash
curl -H "Authorization: Bearer gc-xxxxxxxxxxxxxxxx" \
  "http://localhost:5000/open-api/credit/logs?start=2026-05-01T00:00:00Z&page=1&page_size=20"
```

---

## 对接示例

### Python

```python
import requests

API_KEY = "gc-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
BASE_URL = "http://localhost:5000"
HEADERS = {"Authorization": f"Bearer {API_KEY}"}

# 查询余额
resp = requests.get(f"{BASE_URL}/open-api/credit/balance", headers=HEADERS)
print(resp.json())

# 查询消耗统计
resp = requests.get(f"{BASE_URL}/open-api/credit/usage", headers=HEADERS, params={
    "start": "2026-05-01T00:00:00Z",
    "end": "2026-05-11T23:59:59Z"
})
print(resp.json())

# 查询流水
resp = requests.get(f"{BASE_URL}/open-api/credit/logs", headers=HEADERS, params={
    "page": 1,
    "page_size": 20
})
print(resp.json())
```

### JavaScript / Node.js

```javascript
const API_KEY = 'gc-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx';
const BASE_URL = 'http://localhost:5000';
const headers = { 'Authorization': `Bearer ${API_KEY}` };

// 查询余额
const balance = await fetch(`${BASE_URL}/open-api/credit/balance`, { headers }).then(r => r.json());
console.log(balance);

// 查询消耗统计
const usage = await fetch(`${BASE_URL}/open-api/credit/usage?start=2026-05-01T00:00:00Z`, { headers }).then(r => r.json());
console.log(usage);
```
