# Aether 编译器前端基础设施审查与规划

> 生成时间：2026-07-06
> 审查范围：`crates/aether-core`、`crates/aether-lsp`、`crates/aether-tree-sitter`、`crates/aether-win32`

---

## 一、项目现状总览

| 能力维度 | 当前状态 | 完成度 | 说明 |
|---------|---------|--------|------|
| 词法分析（Lexer） | 已有手写实现 | ~60% | 9 种语言 lexer，覆盖常见 token，但存在性能与边界问题 |
| 语法分析（Parser） | **完全缺失** | 0% | 无 AST 定义，无 parser 模块 |
| 语义分析 | **完全缺失** | 0% | 无符号表、作用域、类型系统 |
| LSP 客户端 | 基础框架已搭 | ~40% | 常见请求已覆盖，但反向请求、诊断缓存、增量同步等缺失 |
| Tree-sitter 集成 | 已引入未接入 | ~30% | 高亮器已实现，但未被主渲染流程调用 |

---

## 二、Lexer 详细审查

### 2.1 已支持语言

| 语言 | 文件 | 备注 |
|------|------|------|
| C/C++ | `c_lexer.rs` | 含 `.m`、`.mm` |
| Rust | `rust_lexer.rs` | |
| Python | `python_lexer.rs` | |
| JavaScript/TypeScript | `js_lexer.rs` | JSX/TSX 未支持 |
| JSON/JSONC/JSONL | `json_lexer.rs` | JSONC 注释未支持 |
| Markdown/MDX | `markdown_lexer.rs` | 围栏代码块结束未识别 |
| TOML/INI/CFG | `toml_lexer.rs` | 多行字符串未支持 |
| HTML/模板类 | `html_lexer.rs` | Vue/Svelte/WXML 等归入此类 |
| CSS/SCSS/SASS/LESS/WXSS | 复用 `html_lexer.rs` | 无独立 CSS lexer |

### 2.2 主要问题

#### 高优先级问题

1. **动态分配与分发**
   - `Language::create_lexer()` 返回 `Box<dyn Lexer>`，每次打开文件都堆分配
   - `lex_full()` 返回 `Vec<LexemeSpan>` 无预分配容量

2. **逐字节扫描**
   - 所有 `skip_*` 函数都是 `while` 单字节推进
   - 未使用 SIMD、`memchr`、查找表等批量扫描技术

3. **重复的 UTF-8 转换**
   - 几乎每个 lexer 在识别标识符后执行 `std::str::from_utf8(...).unwrap_or("")`
   - 仅用于 `matches!` 关键词判断

4. **关键词线性匹配**
   - 使用 `matches!(text, "a" | "b" | ...)`，本质是逐个字符串比较

5. **边界越界风险**
   - 字符串/字符/正则/模板字符串跳过函数中 `if bytes[i] == b'\\' { i += 2; }` 未先判断 `i + 1 < len`

6. **数字解析贪婪**
   - `skip_number` 把 `.`、`-`、`+`、`e`、`E` 全部纳入，导致 `1..2` 等被错误合并

7. **UTF-8 多字节字符处理错误**
   - 中文字符、emoji 等被拆成多个 `Unknown` token，导致高亮错位

#### 中优先级问题

8. Rust 不支持原始字符串 `r#"..."#`、字节字符串 `b"..."`、C 字符串 `c"..."`
9. Python 字符串前缀支持不完整（`r`、`b`、`u`、`f` 组合）
10. JS 正则上下文判断启发式，误判率高
11. Markdown 围栏代码块不识别结束围栏
12. HTML 标签名被标为 `Keyword`，语义不准确

#### 低优先级问题

13. 缺少 CSS 独立 lexer
14. 缺少 JSX/TSX 支持
15. 缺少 Unicode 标识符支持
16. 测试覆盖不足

---

## 三、Parser / AST / 语义分析现状

### 3.1 结论：完全缺失

- 无 `pub mod parser`
- 无 `struct Ast` / `enum Ast`
- 无 `trait Parser`
- 无 `SymbolTable`、`ScopeTree`、`TypeKind`
- 无 `type_check`、`semantic_analyze`

### 3.2 已有可复用资产

- `crates/aether-tree-sitter` 已引入 tree-sitter 0.20 及多种语言 grammar
- `TreeSitterHighlighter` 已实现增量解析、高亮转换
- 但当前未被 `aether-win32` 主渲染流程调用

### 3.3 建议路线

**不要从零手写 parser/AST**。优先基于 tree-sitter 构建：

1. 接入 tree-sitter 到主渲染路径，替代/补充手写 lexer 高亮
2. 在 tree-sitter `Tree`/`Node` 上封装 AST wrapper
3. 基于 AST 构建 `SymbolTable` 和 `ScopeTree`
4. 高级语义（类型检查、重命名）继续委托 LSP；如需本地闭环，引入 `biome`、`oxc` 或 `ra_ap_*`

---

## 四、LSP 客户端（aether-lsp）审查

### 4.1 已实现方法

`LspClient` 已提供以下 `pub async fn`：

- `start_server`
- `open_document` / `close_document`
- `notify_change`
- `request_completion`
- `request_hover`
- `request_definition`
- `request_references`
- `request_rename`
- `request_code_actions`
- `request_formatting`
- `request_semantic_tokens_full` / `delta` / `range`
- `request_inlay_hints`
- `shutdown_all`
- `is_server_ready`
- `get_capabilities`

### 4.2 已实现 LSP 消息

生命周期：`initialize`、`initialized`、`shutdown`、`exit`
文档同步：`textDocument/didOpen`、`didClose`、`didChange`
文本请求：completion、hover、definition、references、rename、codeAction、formatting、semanticTokens、inlayHint
推送通知：`textDocument/publishDiagnostics`、`window/logMessage`

### 4.3 主要缺失

1. **服务器反向请求未响应**
   - `workspace/configuration`、`client/registerCapability`、`workspace/applyEdit` 等只记录日志不回复
   - 会导致服务器请求超时

2. **诊断管理不完整**
   - `DiagnosticCollection` 是死代码
   - 无 Pull Diagnostics
   - 文档关闭/服务器退出时未清理诊断

3. **文档同步质量低**
   - 实际使用行级启发式 diff，非字符级
   - `incremental_sync.rs` 中的优化实现未接入
   - 大文件超过 10000 字符直接全文替换
   - 无 UTF-16 码元转换

4. **能力检查缺失**
   - 发送请求前未检查服务器 capabilities

5. **其他缺失功能**
   - signatureHelp、documentSymbol、documentHighlight、typeDefinition、implementation
   - workspace/symbol、workspace/executeCommand
   - 进度 `$/progress`、取消 `$/cancelRequest`
   - 服务器崩溃重连

### 4.4 完成度评估

- Demo 级客户端（补全/跳转/诊断）：~45%
- 生产级多语言客户端：~30%
- **综合：~40%**

---

## 五、分阶段实施计划

### Phase 1：Lexer 基础优化（当前可落地）

目标：在不改变架构的前提下，修复明显缺陷并提升性能。

| 任务 | 优先级 | 预估工作量 |
|------|--------|-----------|
| 消除 `Box<dyn Lexer>` 动态分配，改为静态分发 | 高 | 小 |
| 修复字符串/注释跳过函数边界越界 | 高 | 小 |
| 提取通用 `skip_*` 工具函数，减少重复代码 | 高 | 中 |
| 关键词匹配改为字节数组 `match` 或 `phf` 静态哈希 | 高 | 小 |
| 预分配 `Vec<LexemeSpan>` 容量 | 中 | 小 |
| 修复数字解析贪婪问题 | 高 | 中 |
| 修复 UTF-8 多字节字符推进（避免高亮错位） | 高 | 中 |

### Phase 2：LSP 客户端补全（当前可落地）

目标：让 LSP 客户端达到可用级别。

| 任务 | 优先级 | 预估工作量 |
|------|--------|-----------|
| 处理服务器反向请求（configuration/registerCapability/applyEdit） | 高 | 中 |
| 实现诊断缓存、清理与 Pull Diagnostics | 高 | 中 |
| 接入 `incremental_sync.rs` 优化版增量同步 | 高 | 大 |
| 增加 UTF-16 ↔ 字节偏移转换 | 高 | 中 |
| 请求前检查服务器 capabilities | 中 | 小 |
| 实现 `textDocument/signatureHelp` | 中 | 小 |
| 实现 `textDocument/documentSymbol` | 中 | 小 |

### Phase 3：Tree-sitter 语义基础（中期）

目标：建立 parser/AST/符号表基础设施。

| 任务 | 优先级 | 预估工作量 |
|------|--------|-----------|
| 将 `aether-tree-sitter` 接入主渲染路径 | 高 | 大 |
| 基于 tree-sitter Tree 封装 AST wrapper | 高 | 大 |
| 实现 `SymbolTable` 与 `ScopeTree` | 高 | 大 |
| 实现本地符号跳转（不依赖 LSP） | 中 | 中 |
| 实现简单的语法错误定位 | 中 | 中 |

### Phase 4：自研 Parser/AST（长期，谨慎投入）

**建议：仅当 Tree-sitter 无法满足特定需求时再启动。**

原因：
- 手写多语言 parser 成本极高
- 维护负担重
- 当前 tree-sitter 生态已覆盖主流语言

---

## 六、性能优化策略

### 6.1 短期（无需架构改动）

1. **静态 lexer 实例**：`static JS_LEXER: JsLexer = JsLexer;`
2. **预分配 token Vec**：`Vec::with_capacity(text.len() / 4)`
3. **字节数组关键词匹配**：避免 `from_utf8` 和字符串 `matches!`
4. **使用 `memchr` 批量扫描**：替换 `skip_whitespace`、`skip_line_comment` 等
5. **增量 lexing 行缓存**：只重新 lex 修改过的行

### 6.2 中期（需要架构调整）

1. **引入 `logos` 生成 lexer**：状态机驱动，性能接近手写 SIMD
2. **Tree-sitter 替代手写 lexer**：复用其增量解析能力
3. **异步延迟解析**：大文件只解析可见区域
4. **并行解析**：多文件项目可并行构建符号表

### 6.3 性能基准建议

| 指标 | 基准文件 | 目标 |
|------|---------|------|
| 单文件 lex 时间 | 10KB Rust 文件 | < 2ms |
| 单文件 lex 时间 | 100KB Rust 文件 | < 10ms |
| 按键响应延迟 | 任意文件 | < 16ms（60fps） |
| 大文件打开 | 1MB 文件 | < 100ms 可编辑 |
| LSP 诊断延迟 | rust-analyzer | < 2s |

---

## 七、已知限制

1. **Parser/AST/语义分析缺失**：短期内无法提供本地类型检查、重命名、查找引用等功能
2. **Lexer 健壮性不足**：未闭合字符串、非法 UTF-8、特殊语法边界等情况处理不完美
3. **LSP 客户端不完整**：服务器反向请求未处理，可能导致语言服务器行为异常
4. **Tree-sitter 未接入**：虽已引入，但当前未产生实际价值
5. **多语言覆盖有限**：CSS、JSX/TSX、Vue 单文件组件等语法高亮较弱
6. **性能未基准化**：缺少自动化性能测试和回归监控

---

## 八、潜在优化方向

1. 用 `logos` 统一生成所有语言 lexer
2. 用 tree-sitter 完全替代手写 lexer 和 parser
3. 引入 `salsa` 等增量计算框架构建本地语义数据库
4. 对 LSP 客户端实现请求队列、去抖、取消机制
5. 建立自动化 lexer/parser 基准测试套件
6. 与 `rust-analyzer` 库（`ra_ap_*`）或 `biome` 深度集成，获得本地语义能力

---

## 九、决策建议

**当前阶段（Phase 1 + Phase 2）应该做：**
- Lexer 基础优化（消除分配、修复边界、优化关键词匹配）
- LSP 客户端补全（反向请求、诊断缓存、增量同步优化）

**当前阶段不应该做：**
- 从零手写完整 parser/AST/语义分析（投入产出比太低）
- 大规模重写 lexer 为状态机/SIMD（可在后续迭代中逐步引入）
- 追求 100% LSP 协议覆盖（优先满足日常开发 80% 场景）

**下一步行动：**
1. 选择 Phase 1 中的高优先级任务开始实施
2. 每完成一个子任务即运行测试与基准
3. 若某任务出现明显瓶颈，记录限制并转入下一任务
