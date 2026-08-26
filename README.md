# Model Relay · 模型中转站

Rust 实现的 LLM API 统一中转网关：一个入口对接多家模型供应商，支持**拼车（多用户共享渠道）**，每人独立令牌、独立额度、用量归因。

## 状态

📐 设计阶段（v0.1 草案）——模块文档已就绪，待开发

## 功能规划

- OpenAI 兼容接口：`/v1/chat/completions`、`/v1/models`
- 多渠道接入：OpenAI / DeepSeek / Anthropic / Gemini / Ollama
- 拼车：全局共享渠道池 + 用户/令牌双层配额 + 用量归因
- 负载均衡 / 故障转移 / 重试
- 令牌鉴权（sha256 哈希存储）、管理端 session + 验证码
- 计费与用量统计
- 管理后台（前后端分离：TypeScript + esbuild）

## 技术栈

| 层 | 技术 |
|----|------|
| 后端 | Rust + axum + tokio + rusqlite（SQLite） |
| 前端 | TypeScript + esbuild + Tailwind |
| 部署 | 单二进制（musl 静态编译）+ 静态文件 |

## 文档

- [模块文档](docs/模块文档.md)：模块划分、数据模型、数据流、部署方案

## 目录结构

```
model-relay/
├── crates/          # Rust workspace（core/store/provider/billing/gateway/api/server）
├── frontend/        # 管理后台 TS 源码
├── public/          # 构建产物 + index.html
├── docs/            # 文档
└── scripts/         # 测试脚本
```
