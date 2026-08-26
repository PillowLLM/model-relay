# 11 管理 API 与前端模块（frontend / relay-api）

> 乙队（前端）独立施工依据。全部接口契约以 14 文档为准，本文档定义页面结构与对接方式。

## 1. 技术栈（与 DocPI 完全一致，团队已有经验）

| 项 | 选型 |
|----|------|
| 语言 | TypeScript（严格模式） |
| 构建 | esbuild（`build.mjs`，`charset:'utf8'`） |
| 样式 | Tailwind CDN + 手写 style.css（纯黑纯白药丸风） |
| 架构 | 原生 TS 模块（state/api/ui/views 分层，无框架） |
| 图标 | 内联 SVG（icons.ts） |

```jsonc
// frontend/package.json
{
  "scripts": { "build": "node build.mjs", "test": "tsc --noEmit && node build.mjs" },
  "devDependencies": { "esbuild": "^0.24", "typescript": "^5.7" }
}
```

## 2. 前端目录结构

```
frontend/
├── build.mjs                # esbuild 构建 + 缓存版本号（?v=时间戳，与 DocPI 同款）
├── tsconfig.json
└── src/
    ├── main.ts              # 入口：初始化 + 路由分发
    ├── state.ts             # 全局状态（user/currentPage/缓存）
    ├── api.ts               # fetch 封装（Cookie 凭证 + JSON + 错误统一处理）
    ├── ui.ts                # 弹窗/toast/确认框/分页组件（DocPI ui.ts 同款）
    ├── icons.ts             # SVG 图标
    ├── views/
    │   ├── login.ts         # 登录页（验证码）
    │   ├── dashboard.ts     # 仪表盘（统计图表）
    │   ├── channels.ts      # 渠道管理
    │   ├── tokens.ts        # 令牌管理
    │   ├── users.ts         # 用户管理
    │   ├── groups.ts        # 分组管理
    │   ├── pricing.ts       # 模型定价
    │   ├── redeem.ts        # 兑换码
    │   ├── usage.ts         # 用量日志
    │   ├── recharge.ts      # 充值（用户自助）
    │   ├── notify.ts        # 通知/支付配置
    │   ├── tokenizers.ts    # 词表管理
    │   └── health.ts        # 系统状态
    └── charts.ts            # 轻量图表（手写 canvas，不引大库）
```

## 3. 页面与路由

SPA 单页：左侧边栏导航 + 右侧内容区。路由用 hash：`#/channels`、`#/users` 等。

| 页面 | 权限 | 调用的 API（见 14 文档） |
|------|------|--------------------------|
| 登录 | 公开 | GET /api/auth/captcha、POST /api/auth/login |
| 仪表盘 | admin | GET /api/admin/stats?days=30 |
| 渠道管理 | admin | B2 全部 |
| 令牌管理 | admin | B3 全部 |
| 用户管理 | admin | B4 users |
| 分组管理 | admin | B4 groups |
| 模型定价 | admin | B5 pricing |
| 兑换码 | admin | B5 redeem |
| 用量日志 | admin | B6 usage |
| 系统状态 | admin | B6 health |
| 词表管理 | admin | B6 tokenizers |
| 通知配置 | admin | B6 notify-config |
| 充值订单 | admin | B6 recharge-orders |
| 我的用量/充值 | user | B7 自助 |

## 4. 前端-后端对接契约（甲乙两队对照）

```ts
// src/api.ts —— 统一请求封装
export async function api<T>(path: string, opts: { method?: string; body?: unknown } = {}): Promise<T> {
  const res = await fetch(path, {
    method: opts.method ?? "GET",
    headers: opts.body ? { "Content-Type": "application/json" } : undefined,
    body: opts.body ? JSON.stringify(opts.body) : undefined,
    credentials: "same-origin",        // Cookie 会话
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const msg = data?.error ?? `HTTP ${res.status}`;
    if (res.status === 401) { location.href = "#/login"; }
    throw new Error(msg);
  }
  return data as T;
}
```

**约定**：
- 列表接口返回 `{items: [], total: n}`；前端用 ui.ts 的分页组件
- 错误消息直接 toast 展示（`data.error`）
- 渠道 API Key：创建时输入，列表只显示 `api_key_masked`
- 令牌明文：创建响应里的 `token` 字段**只出现一次**，前端弹窗「仅显示一次，请立即复制」
- 统计图表：`GET /api/admin/stats` 返回按天/按模型数组，charts.ts 画柱状/折线

## 5. 构建与缓存（沿用 DocPI 已验证方案）

```js
// build.mjs 关键点
import { build } from 'esbuild';
await build({ entryPoints: ['src/main.ts'], bundle: false, outdir: '../public/js', charset: 'utf8' });
// 构建后给所有 import 引用 + index.html 入口追加 ?v=<时间戳>（防浏览器缓存）
```

## 6. 乙队验收清单

- [ ] 登录/验证码/登出流程通
- [ ] 渠道 CRUD + 批量导入 + 测试按钮
- [ ] 令牌创建（明文一次）→ 客户端用该令牌能转发（联动测试）
- [ ] 用户/分组/定价/兑换码 CRUD
- [ ] 仪表盘统计渲染
- [ ] 系统状态页（health 报告展示）
- [ ] 兑换/充值流程（乙队用 mock 回调即可）
