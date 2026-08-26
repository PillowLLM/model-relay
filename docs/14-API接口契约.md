# 14 API 接口契约（完整）

> 本文档是前后端联调与客户端接入的**唯一权威契约**。所有接口遵循：
> - 请求/响应均为 JSON（UTF-8），`Content-Type: application/json`
> - 错误统一：`{"error": "消息", "code": <HTTP状态>}`
> - 时间格式：`"YYYY-MM-DD HH:MM:SS"`
> - 金额：元（REAL）；价格：元/1M token；配额：token 数

## A. 转发 API（客户端调用，OpenAI 兼容）

### A1. 认证方式

| 方式 | 示例 |
|------|------|
| Header | `Authorization: Bearer sk-xxxxxxxxxxxxxxxxxxxxxxxx` |
| Query | `?token=sk-xxxxxxxxxxxxxxxxxxxxxxxx` |

令牌格式：`sk-` + 24 位 `[a-zA-Z0-9]`（共 27 字符）。校验失败返回 401。

### A2. `GET /v1/models` — 模型列表

```http
GET /v1/models
Authorization: Bearer sk-xxx
```

```json
// 200
{
  "object": "list",
  "data": [
    { "id": "gpt-4o", "object": "model", "owned_by": "relay" },
    { "id": "deepseek-chat", "object": "model", "owned_by": "relay" }
  ]
}
```
模型列表 = 全部启用渠道的 models 并集（经 model_mapping 反向展开）。

### A3. `POST /v1/chat/completions` — 对话（核心）

```http
POST /v1/chat/completions
Authorization: Bearer sk-xxx
Content-Type: application/json
```

**请求体字段约束**：

| 字段 | 类型 | 必填 | 约束 |
|------|------|------|------|
| `model` | string | ✅ | ≤64 字符 |
| `messages` | array | ✅ | 1–512 条 |
| `messages[].role` | string | ✅ | `system`/`user`/`assistant`/`tool` |
| `messages[].content` | string\|array | ✅ | 文本 ≤128KB；数组支持 `{"type":"text","text":...}` 与 `{"type":"image_url","image_url":{"url":"..."}}` |
| `max_tokens` | int | 否 | 1–1,000,000；缺省 4096 |
| `temperature` | float | 否 | 0–2，缺省 1 |
| `top_p` | float | 否 | 0–1 |
| `stream` | bool | 否 | 缺省 false |
| `stop` | array | 否 | ≤4 项，每项 ≤64 字符 |
| `tools` | array | 否 | ≤128 项 |
| `tool_choice` | object\|string | 否 | `"auto"`/`"none"`/`{"type":"function","function":{"name":"..."}}` |
| `user` | string | 否 | 透传 |

**非流式示例**：

```bash
curl https://relay.yxpil.com/v1/chat/completions \
  -H "Authorization: Bearer sk-xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "你好"}],
    "max_tokens": 512,
    "stream": false
  }'
```

```json
// 200（非流式）
{
  "id": "chatcmpl-8f3a...",
  "object": "chat.completion",
  "model": "gpt-4o",
  "choices": [{
    "index": 0,
    "message": { "role": "assistant", "content": "你好！有什么可以帮你？" },
    "finish_reason": "stop"
  }],
  "usage": { "prompt_tokens": 4, "completion_tokens": 9, "total_tokens": 13 }
}
```

**流式示例**（`stream: true`，规范见第 C 节）：

```http
HTTP/1.1 200
Content-Type: text/event-stream
```

### A4. `GET /v1/dashboard/billing/subscription` — 额度查询（NEWAPI 同款）

```bash
curl https://relay.yxpil.com/v1/dashboard/billing/subscription \
  -H "Authorization: Bearer sk-xxx"
```

```json
// 200
{
  "object": "billing_subscription",
  "hard_limit_usd": 100,
  "system_hard_limit_usd": 100,
  "soft_limit_usd": 100
}
```
`hard_limit_usd = quota_limit / 1000`（以 1 元 = 1000 token 折算展示，仅前端显示用）。

### A5. `GET /v1/dashboard/billing/usage` — 用量查询

```bash
curl "https://relay.yxpil.com/v1/dashboard/billing/usage?start_date=2026-08-01&end_date=2026-08-31" \
  -H "Authorization: Bearer sk-xxx"
```
```json
{ "object": "list", "total_usage": 12345, "daily_costs": [{"timestamp": 1724601600, "line_items": [{"name": "gpt-4o", "amount": 0.01}]}] }
```

### A6. A社格式入口 `POST /v1/messages`

客户端用 A社格式（`x-api-key` 或 `Authorization: Bearer` 均可，`anthropic-version` 头可选）：
```bash
curl https://relay.yxpil.com/v1/messages \
  -H "x-api-key: sk-xxx" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3-5-sonnet",
    "max_tokens": 512,
    "system": "你是助手",
    "messages": [{"role": "user", "content": "你好"}]
  }'
```
入站经 AUTOFIT 识别 → canonical → 按渠道类型出站。**响应也按 A社格式返回**（`content:[{type:"text",text:...}]`、`usage:{input_tokens,output_tokens}`）。

### A7. 转发错误码

| 码 | 场景 | 响应体 |
|----|------|--------|
| 401 | 令牌无效/禁用/过期 | `{"error":"令牌无效或已过期"}` |
| 403 | 账号禁用 | `{"error":"账号已禁用"}` |
| 429 | 限流/额度不足 | `{"error":"额度不足"}` 或 `{"error":"请求过于频繁"}`，头 `Retry-After` |
| 5xx | 上游全部失败 | `{"error":"所有渠道均失败"}` |
| 502 | 上游返回错误 | `{"error": <上游错误消息>}`（透传） |

---

## B. 管理 API（管理后台，Session 认证）

> 除登录/验证码外全部需 Cookie `relay_session`，非 admin 返回 403。分页统一 `?page=1&page_size=20`。

### B1. 认证

| 接口 | 说明 |
|------|------|
| `GET /api/auth/captcha` | `{"id","image"}`（image 为 data URL PNG） |
| `POST /api/auth/login` | `{username,password,captcha_id,captcha}` → `{user}`；Set-Cookie |
| `POST /api/auth/logout` | 清 session |
| `GET /api/auth/me` | 当前用户（含 group/quota 信息） |
| `PUT /api/auth/me` | `{display_name,email,phone,password?}` 改资料/密码 |
| `PUT /api/auth/me/avatar` | `{avatar: "data:image/png;base64,..."}`（≤2MB） |

### B2. 渠道

| 接口 | 请求体要点 | 返回 |
|------|-----------|------|
| `GET /api/admin/channels` | `?page&type&keyword` | `{items:[{...,api_key_masked}],total}` |
| `POST /api/admin/channels` | `{name≤32,type,base_url≤256,api_key≤512,models,model_mapping,priority(-10..10),weight(1..100),group_ids"1,2",auto_ban,max_retries(0..5),timeout_ms(1000..300000)}` | 新建渠道 |
| `PUT /api/admin/channels/{id}` | 同创建（api_key 空=不改） | `{ok:true}` |
| `DELETE /api/admin/channels/{id}` | — | `{ok:true}` |
| `PUT /api/admin/channels/{id}/status` | `{status:0\|1}` | `{ok:true}` |
| `POST /api/admin/channels/batch` | `[{...},...]`（≤100 条） | `{success:n,failed:[{index,error}]}` |
| `POST /api/admin/channels/{id}/test` | — | `{ok:true,latency_ms,models_count}` 或 400 |

### B3. 令牌

| 接口 | 说明 |
|------|------|
| `GET /api/admin/tokens?user_id=&page=` | 列表（不含明文） |
| `POST /api/admin/tokens` | `{user_id,name≤32,quota_limit(-1不限),expired_at}` → `{token:"sk-xxx",...}` 明文仅一次 |
| `PUT /api/admin/tokens/{id}` | `{name,quota_limit,expired_at}` |
| `DELETE /api/admin/tokens/{id}` | `{ok:true}` |
| `PUT /api/admin/tokens/{id}/status` | `{status:0\|1}` |

### B4. 用户 / 分组

| 接口 | 说明 |
|------|------|
| `GET/POST/PUT/DELETE /api/admin/users` | 用户 CRUD；POST `{username(3-32),password(8-64),role,group_id,quota_limit,email,phone,display_name}` |
| `GET/POST/PUT/DELETE /api/admin/groups` | 分组 CRUD；POST `{name,quota_multiplier(0.1..10),remark}` |

### B5. 定价 / 兑换码

| 接口 | 说明 |
|------|------|
| `GET /api/admin/pricing` | 全部定价 |
| `POST /api/admin/pricing` | `{model≤64,prompt_price,completion_price}`（upsert） |
| `DELETE /api/admin/pricing/{id}` | — |
| `GET /api/admin/redeem?page=&status=` | 兑换码列表 |
| `POST /api/admin/redeem` | `{count(1..100),quota,expires_at?}` → `{codes:[...]}` 批量生成 |
| `PUT /api/admin/redeem/{id}/status` | `{status:0\|1\|2}`（禁用/启用） |

### B6. 词表 / 通知配置 / 充值

| 接口 | 说明 |
|------|------|
| `POST /api/admin/tokenizers/{family}` | multipart 上传（字段 `file`，≤50MB，见 15 文档文件标准） |
| `GET /api/admin/tokenizers` | 已上传词表列表 |
| `DELETE /api/admin/tokenizers/{family}` | — |
| `GET/PUT /api/admin/notify-config` | 短信/邮箱/支付配置（密钥不回显） |
| `GET /api/admin/recharge-orders?status=` | 订单列表 |
| `PUT /api/admin/recharge-orders/{id}/confirm` | 手动确认到账（manual 方式） |
| `GET /api/admin/health` | 自检报告（见 10 文档） |
| `GET /api/admin/usage?user_id=&channel_id=&model=&start=&end=&page=` | 用量日志 |
| `GET /api/admin/stats?days=30` | 统计：`{total_tokens,total_cost,requests,by_model:[],by_day:[],by_channel:[]}` |

### B7. 用户自助

| 接口 | 说明 |
|------|------|
| `POST /api/redeem` | `{code:"REDEEM-XXXX-XXXX-XXXX-XXXX"}` 兑换 → 加额度 |
| `GET /api/recharge/plans` | 充值档位（settings 配置） |
| `POST /api/recharge/order` | `{plan_id,pay_method}` → `{order_no,qr_url}` |
| `GET /api/recharge/orders` | 我的订单 |
| `GET /api/usage/my?start=&end=` | 我的用量 |

---

## C. SSE 流式规范

### C1. 通用规则

- `Content-Type: text/event-stream; charset=utf-8`
- 每事件：`data: {json}\n\n`（A社格式额外带 `event: <type>` 行）
- **必须关闭代理缓冲**（nginx `proxy_buffering off`），否则流式不实时
- 心跳：每 15s 空闲发 `: ping` 注释行（nginx 层 `proxy_read_timeout` 建议 300s）
- 终止：OpenAI `data: [DONE]\n\n`；A社 `event: message_stop`
- 错误：终止前发 `data: {"error":{"message":"...","code":...}}` 然后终止

### C2. OpenAI 事件流（出站/标准）

```
data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

### C3. A社事件流（A社客户端入站时）

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_xxx","content":[],"usage":{"input_tokens":4,"output_tokens":0}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}
```

### C4. AUTOFIT 流式转换规则

| 上游 | 客户端 OpenAI | 客户端 A社 |
|------|--------------|-----------|
| OpenAI delta.content | 透传 | → `content_block_delta(text_delta)` |
| OpenAI delta.tool_calls | 透传 | → `content_block_delta(input_json_delta)` |
| OpenAI [DONE] | 透传 | → `message_stop` |
| A社 text_delta | → `choices[0].delta.content` | 透传 |
| A社 message_start | 丢弃 | 透传 |
| A社 usage | → 追加 usage 字段 | 透传 |
| 用量统计 | 首个 usage/终止事件时结算 | 同左 |

### C5. 流式计费

- 上游 A社：`message_delta.usage` 结算
- 上游 OpenAI：`choices[0].delta.usage` 或终止前补发 usage 块；缺失 → relay-tokenizer 按已收文本兜底计数
- 结算后立刻 `billing.record_usage` + 落日志（即使连接中断，已收文本也结算）

---

## D. 连接方式与超时

| 项 | 值 |
|----|-----|
| 协议 | HTTP/1.1 + keep-alive；TLS 1.2+ |
| 转发 API 超时 | 非流式：渠道 timeout_ms（默认 60s）；流式：首个 chunk 30s，chunk 间 60s |
| 管理 API 超时 | 30s |
| 客户端并发 | 每令牌并发 ≤10（超限 429） |
| WebSocket | 讯飞星火专用（M5，独立连接器） |
| nginx 反代 | `proxy_buffering off; proxy_read_timeout 300s; proxy_http_version 1.1` |
