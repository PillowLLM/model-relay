# 06 Token 计数模块（relay-tokenizer）

## 1. 职责

- 本地 BPE token 计数（tiktoken 同款算法），不依赖上游
- 词表参考文件上传管理（.tiktoken / .bpe）
- 为 billing 提供**前置用量估计**与**后置兜底计数**

## 2. 数据结构

```rust
// crates/relay-tokenizer/src/lib.rs
pub trait Tokenizer: Send + Sync {
    fn family(&self) -> &str;
    fn count(&self, text: &str) -> usize;
    fn encode(&self, text: &str) -> Vec<u32>;
}

pub struct BpeTokenizer {
    family: String,
    // 编码表：token_bytes -> id（BPE 合并查找表）
    encoder: HashMap<Vec<u8>, u32>,
    // 解码表：id -> token_bytes（encode 时用贪心最大匹配）
    decoder: HashMap<u32, Vec<u8>>,
}

pub struct TokenizerRegistry { map: RwLock<HashMap<String, Arc<dyn Tokenizer>>> }
impl TokenizerRegistry {
    pub fn with_builtin() -> Self;      // 内置 cl100k_base / o200k_base / llama / qwen / gpt2
    pub fn register_file(&self, family: &str, data: &[u8]) -> Result<(), AppError>;  // 解析+注册
    pub fn get(&self, family: &str) -> Result<Arc<dyn Tokenizer>, AppError>;
    pub fn default_family(&self) -> String;
}
```

## 3. BPE 编码算法（核心实现）

```rust
impl Tokenizer for BpeTokenizer {
    fn encode(&self, text: &str) -> Vec<u32> {
        // 1. 字节级预分词（regex 或简单 Unicode 分裂）：
        //    tiktoken 用 GPT2_SPLIT_PATTERN；简化实现按 字符/空白/标点 分组
        // 2. 每段字节序列做贪心合并：
        //    while 可合并:
        //      找相邻对 (a,b)，查 encoder 是否有合并键 [a,b]（即合并后 token 存在）
        //      选最高分（先按 encoder 直接包含，再按字节模式）
        // 3. 输出 id 序列
        let mut ids = Vec::new();
        for seg in pretokenize(text) {
            ids.extend(self.merge_segment(&seg));
        }
        ids
    }
    fn count(&self, text: &str) -> usize { self.encode(text).len() }
}
```

> **实现提示**：完整 tiktoken 合并优化复杂，MVP 可用「逐对贪心合并」O(n²) 版本（计数用途足够）；参考开源实现（如 `rsk-tiktoken` 思路）但保持自研可维护。

## 4. 文件解析与校验

```rust
// crates/relay-tokenizer/src/parse.rs
pub fn parse_tiktoken(data: &[u8]) -> Result<HashMap<Vec<u8>, u32>, String> {
    // 每行: "<id> <base64bytes>"
    // 校验：行数>=1000、id 唯一且连续(0..n)、base64 可解码
}
pub fn parse_bpe(data: &[u8]) -> Result<HashMap<Vec<u8>, u32>, String> {
    // 每行: "<rank> <token>"；token 支持字面字节或 \uXXXX 转义
}
pub fn validate_file(family: &str, data: &[u8]) -> Result<(), String> {
    // family 白名单：cl100k_base/o200k_base/llama/qwen/gpt2/custom
    // 大小 <= 50MB；格式按 family 选择 parse_tiktoken / parse_bpe
}
```

## 5. 管理 API（见 14 文档 B6）

- `POST /api/admin/tokenizers/{family}`：multipart 上传（字段 `file`），流程：
  1. 校验大小/格式 → 400 不通过
  2. 计算 sha256 → 存 `tokenizer_files` 表
  3. `registry.register_file(family, data)` 热更新（RwLock 写）
- `GET /api/admin/tokenizers`：列表（family/filename/checksum/created_at）
- `DELETE /api/admin/tokenizers/{family}`：删除（内置族不可删）

## 6. 与其它模块联合

### 6.1 前置用量估计（billing 调用）

```rust
// relay-billing/src/lib.rs
pub async fn estimate_cost(&self, req: &CanonicalRequest, user_group: &Group) -> Result<CostEstimate> {
    // 1. 取定价：pricing.get(model)，缺省 0
    // 2. 估算 prompt_tokens：
    //    - 文本：tokenizer.count(拼接所有文本 content + system)
    //    - 图片：每图计 1000 token（cl100k 惯例：低清晰度 85*4 高清晰度 170*4；简化 1000）
    // 3. 估算 completion_tokens：max_tokens（上限估计）
    // 4. cost = (prompt/1M)*prompt_price + (completion/1M)*completion_price
    // 5. × group.quota_multiplier
}
```

### 6.2 后置兜底计数（上游无 usage）

```rust
// relay-gateway 结算时：
if resp.usage.is_some() { 用上游 usage }
else {
    let prompt = tokenizer.count(请求文本总量);
    let completion = tokenizer.count(响应文本);
    usage = TokenUsage { prompt_tokens: prompt, completion_tokens: completion, total_tokens: prompt+completion };
}
```

### 6.3 自检联动（relay-health）

- 启动：遍历 `tokenizer_files` → `validate_file` 重校验（防磁盘损坏）
- 不一致（checksum 不符）→ 自检红灯 + 日志警告

## 7. 本模块测试要求

- [ ] encode/count：对固定文本断言 token 数（与 Python tiktoken 对拍，误差 ≤1%）
- [ ] parse_tiktoken / parse_bpe：正常/空文件/坏行/超大
- [ ] 上传 API：成功/超限/坏格式/覆盖更新
- [ ] estimate_cost：含图片/无定价/组倍率
- [ ] 兜底计数：mock 上游无 usage 场景
