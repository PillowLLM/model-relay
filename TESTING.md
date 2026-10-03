# 测试说明 (model-relay)

本工作区测试分为两类：**单元测试**（crate 源码内 `#[cfg(test)]`，原有）与
**集成测试**（各 crate `tests/` 目录，本次新增）。

## 运行方式

```powershell
# 全工作区
cargo test

# 仅本次新增的测试所在 crate
cargo test -p relay-core -p relay-autofit -p relay-provider -p relay-store
```

要求 Rust 工具链 edition 2021（开发环境 cargo 1.99）。`rusqlite` 为 bundled（自带编译 SQLite），无需外部数据库。

## 本次新增的集成测试

| 文件 | 覆盖 | 类型 |
|------|------|------|
| `crates/relay-core/tests/util_test.rs` | `gen_token`/`gen_redeem_code`/`sha256_hex`/`parse_group_ids`/`mask_key`/`base64_encode` | 单元（集成入口） |
| `crates/relay-autofit/tests/parse_test.rs` | 入站识别 `detect_inbound` + OpenAI/Anthropic 解析 | 单元 + 注入 |
| `crates/relay-provider/tests/registry_test.rs` | Provider 注册表、`usable_channels` 过滤、加权轮询选择器、模型映射 | 钩子/插件 |
| `crates/relay-store/tests/store_test.rs` | 真实 SqliteStore：用户 CRUD、事务兑换、默认分组保护、KV、分页 | 集成 + 注入 |

### 注入测试（不可信输入）
- **relay-store / SQL 注入**：以 `' OR '1'='1` 作为用户名查询，因全程参数化绑定而**返回空**（不返回全部用户）；
  以 `admin'); DROP TABLE users;--` 作为用户名写入，被当作**字面文本**存储，且 `users` 表仍完好可查。
- **relay-autofit / 畸形载荷**：缺失/空/超 512 条 messages、超 64 字符 model、content 类型错误、
  损坏的 tool_calls arguments——均被拒绝或容错为 Null，绝不 panic；XSS 字符串作为不透明数据透传。
- **relay-provider / 模型映射**：损坏 JSON 的 `model_mapping` 回退为透传，不 panic。

### 钩子/插件测试
- **Provider 注册表**：已注册类型（openai/anthropic）可解析，未注册类型被拒绝；
  `usable_channels` 按状态/分组授权/模型白名单过滤。
- **ChannelSelector**：候选去重、加权轮询、cursor 轮换起点。

## 说明
- store 集成测试使用系统临时目录下独立 DB 文件（按进程 id + 标签命名），结束后清理，不触碰任何真实数据。
- 上游真实 HTTP 转发（reqwest）需要网络与真实渠道密钥，不在自动化测试范围。
