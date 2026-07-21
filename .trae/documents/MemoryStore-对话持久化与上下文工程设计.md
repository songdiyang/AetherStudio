# MemoryStore：对话持久化与 ACE 上下文工程设计

> 替代旧《AetherDB 定制向量数据库设计》。自研 mmap/HNSW 引擎已废弃，
> 当前架构以 **MemoryStore 适配层 + SQLite** 为核心。

## 1. 设计目标

1. AI 对话历史的可靠持久化（崩溃不丢数据）
2. 大模型可检索的语义索引（向量 + 关键词混合检索）
3. ACE（Agentic Context Engineering）权重沉淀：把对话中可复用的经验
   沉淀为带 helpful/harmful 计数的策略条目，并在后续对话中注入
4. 存储引擎可整体替换（SQLite → Qdrant Edge / LanceDB / 云端）

## 2. 分层架构

```
对话产生
  ├─ 热：内存状态 + mmap 增量日志（ai_hot_data.rs）
  │      %CONFIG%/Aether/conversations/hot/{conv_id}.log
  │
  ├─ 触发：空闲 30s（5s 定时器 AI_ARCHIVE）/ 关闭会话标签
  │
  ├─ 温：SQLite 主库（ai_warm_data.rs 后台线程异步归档，幂等）
  │      %CONFIG%/Aether/conversations/aether_memory.db (WAL)
  │      ├─ conversations / messages（历史列表 + 恢复）
  │      ├─ vec_messages / vec_bullets（sqlite-vec 向量索引）
  │      ├─ messages_fts（FTS5 trigram 全文索引）
  │      ├─ playbook_bullets（ACE 策略条目 + 权重计数）
  │      └─ prune_log（剪枝审计）
  │
  └─ 归档成功 → 清脏标记 + 删除热日志
```

### 模块职责

| 模块 | 职责 |
|---|---|
| `memory_store.rs` | 存储适配层：`MemoryStore` trait + `SqliteMemoryStore` + `JsonlSessionLog` |
| `embedding.rs` | 文本向量化：ONNX（bge-small-zh-v1.5，512 维），缺失时 n-gram 回退 |
| `ai_hot_data.rs` | 热数据：内存会话状态 + mmap 追加日志 + 脏标记/空闲判定 |
| `ai_warm_data.rs` | 温数据：异步归档、历史加载、语义搜索、workspace 绑定、Reflector 钩子 |
| `reflector.rs` | ACE Reflector prompt / 解析 / 确定性 Curator / playbook 注入文本 |
| `ai_panel.rs` | 会话管理、playbook 注入与反馈归因、Playbook 面板状态 |

## 3. 关键设计决策

### 3.1 为什么放弃自研引擎（旧 AetherDB）

- 持久化正确性是数据库最难的部分：旧引擎存在偏移计算 bug、
  重启后标量索引丢失、无 WAL 崩溃恢复
- 对话持久化不需要向量索引：数据量小时暴力余弦即可，
  索引应是可插拔层而非存储层
- 业界验证：VS Code（JSONL + SQLite KV）、Cursor（两级 SQLite KV）、
  Trae（SQLite KV）全部使用朴素方案

### 3.2 MemoryStore 适配层

上层只依赖 `MemoryStore` trait（会话/消息/playbook/检索/剪枝五组操作），
当前实现 `SqliteMemoryStore`：

- rusqlite **bundled**：SQLite 编译进 exe，零部署
- `journal_mode=WAL` + `synchronous=NORMAL`：崩溃安全 + 读写并发
- **sqlite-vec**：静态链接注册为 auto extension，`vec0` 虚拟表存向量，
  rowid 与业务表一一对应
- **FTS5 trigram**：中英文子串匹配，触发器与 messages 表自动同步

归档幂等：消息 ID = `{conv_id}:{msg_index}`，重复归档 `INSERT OR REPLACE`，
不产生重复数据。

### 3.3 ACE 权重沉淀闭环

```
Generator：用户对话（现有 agent 循环）
   ↑ 注入：search_playbook(query, 5) → system 消息（带 +有效/-无效 计数）
   ↓ 记录：used_bullet_ids 存会话槽位
归档：空闲/关标签 → SQLite
   ↓
Reflector：LLM 反思会话轨迹，提炼 JSON 策略条目
   ↓
Curator：纯代码确定性合并（禁止 LLM 改写存量，防 context collapse）
   ├─ 向量近邻（L2 < 0.45 ≈ 余弦 0.9）→ helpful_count + 1
   └─ 新条目 → 插入 playbook_bullets
反馈：采纳编辑 → helpful++；拒绝编辑 → harmful++
```

设计依据（arXiv:2510.04618）：
- 条目化增量更新，禁止整体重写（context collapse 实测：18282 token → 122 token）
- helpful/harmful 计数器作为"权重"，注入文本中向模型暴露可信度
- 合并逻辑确定性、无 LLM，可批量并行

### 3.4 混合检索（FTS5 + 向量 RRF）

`hybrid_search_messages`：
1. FTS5 bm25 排名（trigram，≥3 字符启用）
2. sqlite-vec KNN 排名
3. RRF 融合：`score = Σ 1/(60 + rank)`（60 为标准平滑常数）
4. conv 过滤 + top-k

### 3.5 Grow-and-refine 剪枝

- 候选条件：`helpful+harmful ≥ min_total_uses(5)` 且
  `harmful ≥ harmful_threshold(3)` 且 `harmful > helpful`
- `dry_run` 预览；审计先写 `prune_log`（内容快照+计数+原因）再删除
- 自动：归档线程每次启动执行一次；手动：`WarmDataStore::prune_bullets`

## 4. 文件与配置

```
%CONFIG%/Aether/
├── conversations/
│   ├── aether_memory.db       # SQLite 主库（WAL）
│   └── hot/{conv_id}.log      # 热数据 mmap 增量日志
└── models/
    └── bge-small-zh-v1.5/
        ├── model.onnx         # 建议 int8 量化（~30MB）
        └── tokenizer.json
```

嵌入模型缺失时自动回退 n-gram 哈希向量（链路可用，语义质量有限），
不阻塞启动。

## 5. 后续方向

- 反馈归因精细化：区分"采纳编辑"与"任务整体成功"的信用分配
- 检索质量评测：用真实对话建评测集，量化 RRF 相对纯向量的提升
- 条目版本历史（当前 update 保 ID 保计数，无历史快照）
- 跨设备同步：MemoryStore 换云端实现（trait 不变）
