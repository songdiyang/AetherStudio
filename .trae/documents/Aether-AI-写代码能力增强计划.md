# Aether —— 从 VS Code 的 AI 设计中汲取精华的落地计划

## 1. 概要

本计划聚焦 **“AI 优先 + 必要编辑基础”**，以 **Chat / Agent 编辑** 作为核心交互形态，且 **不兼容 VS Code 扩展、纯 Rust/WASM 自研**。目标是把 Aether 从“带侧边栏聊天窗口的代码编辑器”升级为具备 **流式对话、上下文附件、Agent 批量改文件 + Diff 预览、全局搜索、LSP 诊断联动** 的 AI 原生编辑器。

计划分 4 个阶段推进，优先完成 Phase 1~2 即可让 AI 写代码体验产生质变；Phase 3~4 提供让 AI 能稳定落地的基础设施与持久化能力。

## 2. 当前状态分析

Aether 的骨架已经比较完整：

- **编辑核心**：`aether-core` 的 Piece Table、增量词法器、多语言高亮、undo/redo 已可用。
- **UI 层**：`aether-win32` 已有窗口、布局、标签页、命令面板、活动栏、AI 面板、设置面板、终端、Git/SSH 等。
- **语言协议**：`aether-lsp` 已实现对 LSP 服务器的启动、同步、补全、hover、定义、引用、重命名、格式化、语义令牌、内联提示等请求；`LspEvent::Diagnostics` 已定义但 UI 未消费。
- **AI 传输**：`aether-ai` 已支持 OpenAI/Claude/Kimi/Azure/Custom 的同步 HTTP 调用，并带 SSRF 防护。

与“AI 写代码”直接相关的明显缺口：

| 缺口 | 当前状态 | 影响 |
|---|---|---|
| **流式响应** | `AiPanel.background_result` 是一次性 `Arc<Mutex<Option<Result<String, String>>>>`（[ai_panel.rs:146](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-win32/src/ai_panel.rs#L146)） | 聊天是整段等待，没有打字机效果，也无法中断 |
| **上下文附件** | prompt 只传简单选中代码或用户输入 | 模型看不到当前文件、打开文件、诊断、项目结构 |
| **Agent 编辑** | 只有 `apply_ai_code` 能在光标/选区插入（[editor.rs:4530](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-win32/src/editor.rs#L4530)） | 无法多文件修改、无法预览、无法撤销批量变更 |
| **Diff 预览** | 无 diff 视图 | AI 修改不敢直接应用 |
| **全局搜索** | 命令面板有入口但无实现 | Agent 无法检索代码库 |
| **LSP 诊断 UI** | `DiagnosticCollection` 已定义（[types.rs:75](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-lsp/src/types.rs#L75)），但 UI 未渲染 | AI 无法基于错误修复，用户也看不到波浪线 |
| **模型参数** | `AiSettings` 只有 provider/key/url/model（[settings.rs:22](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-shared/src/settings.rs#L22)） | 无法调 temperature、system prompt、max_tokens |
| **会话持久化** | 聊天记录仅存内存 | 切换窗口或重启后丢失 |

## 3. 范围与取舍

### 3.1 本次做

- 流式 AI 聊天与上下文附件
- Agent 单轮/多轮编辑 + Diff 预览 + 接受/拒绝
- 全局搜索（文本）
- LSP 诊断 UI + “AI 修复”联动
- AI 设置增强（temperature、max_tokens、system prompt）
- 会话持久化

### 3.2 本次不做

- 真实 AI Inline Completion（Tab 补全）。用户最看重 Chat/Agent，且当前 `inline_completion.rs` 已有基于前缀的占位实现；可后续替换。
- VS Code 扩展 API 兼容。按用户选择，保持纯 Rust/WASM 自研。
- 完整的 MCP / Tool / Remote Agent Host。这些对高级 Agent 有价值，但会显著扩大范围；本次用简单的标记语法实现 Agent 编辑。
- 远程容器 / WSL / 端口转发。SSH 已可用，容器后端仍是占位，不在本次解决。

## 4. 具体改动

### Phase 1：流式聊天 + 上下文附件

让聊天从“一次性等待”变成“实时流式”，并给模型提供正确的上下文。

#### 4.1 `aether-ai`：新增流式 API

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-ai\src\lib.rs`
- **改动**：
  - 新增 `pub enum AiStreamEvent { Token(String), Done, Error(String) }`。
  - 新增 `pub fn chat_completion_stream(&self, messages) -> Result<mpsc::Receiver<AiStreamEvent>, AiError>`。
  - 使用 `ureq` 的 `into_reader()` 读取 SSE 流，按 `data:` 行解析 OpenAI 兼容格式；Claude 流式格式单独分支处理。
  - 保留原有 `chat_completion` 非流式接口，避免破坏现有测试与设置面板的“测试连接”。

#### 4.2 设置增强：模型参数

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-shared\src\settings.rs`
- **改动**：在 `AiSettings` 中新增：
  - `temperature: Option<f32>`
  - `max_tokens: Option<u32>`
  - `system_prompt: Option<String>`
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\settings.rs`
- **改动**：
  - `SettingsField` 增加 `Temperature`、`MaxTokens`、`SystemPrompt`。
  - `SettingsPanel` 增加对应字段与命中区。
  - `to_ai_settings()` 生成新的 `AiSettings`。

#### 4.3 `AiPanel`：支持流式状态

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_panel.rs`
- **改动**：
  - 把 `background_result: Arc<Mutex<Option<Result<String, String>>>>` 替换为：
    ```rust
    pub struct AiStreamState {
        pub partial: String,
        pub done: bool,
        pub error: Option<String>,
    }
    pub background_stream: Arc<Mutex<AiStreamState>>,
    ```
  - `send_message` / `send_quick_action` 启动后台线程，使用新的 `chat_completion_stream`，每收到一个 token 就更新 `partial`。
  - `check_background_result`（[ai_panel.rs:351](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-win32/src/ai_panel.rs#L351)）改为把 `partial` 追加到正在生成的助手消息，并在 `done` 后结束 `is_generating`。
  - 渲染时若 `is_generating` 且最后一条是助手消息，显示闪烁光标或 “● 生成中” 提示。

#### 4.4 上下文附件系统

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_context.rs`
- **内容**：
  ```rust
  pub enum AiContextAttachment {
      CurrentFile,
      Selection,
      OpenFiles,
      Diagnostics,
      FileTree,
      CustomText(String),
  }
  ```
  - 每个附件提供 `to_prompt_fragment(&EditorState) -> String`，输出带路径/语言标记的文本块。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\editor.rs`
- **改动**：新增 `gather_context(&self, attachments: &[AiContextAttachment]) -> String`，读取：
  - 当前活动标签页文件路径与内容；
  - 选区（`selection_start`/`selection_end`）转换为行列文本；
  - 打开文件列表与内容摘要（限制总 token）；
  - LSP 诊断（从 `EditorState.diagnostics` 读取，见 Phase 3）；
  - `FileTree` 快照（最近修改的文件优先）。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_panel.rs`
- **改动**：
  - 新增 `attachments: Vec<AiContextAttachment>`。
  - UI 上渲染附件 chips（如“当前文件”、“选区”）。
  - 新增 `@` 快捷键或工具栏按钮弹出附件选择菜单（复用 `command_palette` 的过滤逻辑）。
  - 发送前调用 `EditorState.gather_context()` 拼接到 system/user prompt。

#### 4.5 提示词构建器

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_prompt.rs`
- **内容**：
  - `build_chat_prompt(settings: &AiSettings, context: &str, user_input: &str, mode: AiMode) -> Vec<ChatMessage>`
  - `AiMode` 枚举：`Ask`、`Edit`、`Agent`。
  - `Edit`/`Agent` 模式下追加固定指令，要求模型按标记格式输出修改（见 Phase 2）。
  - 使用 `settings.system_prompt` 作为 system 消息，若为空则使用默认中文 system prompt。

### Phase 2：Agent 编辑 + Diff 预览

让 AI 不只是“生成代码”，而是能安全地批量修改项目文件。

#### 4.6 AI 响应解析器

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_agent.rs`
- **内容**：
  - `AiEdit { path: PathBuf, search: String, replace: String }`。
  - 解析标记语法：
    ```text
    <<<<<<< FILE src/main.rs >>>>>>>
    ...old content...
    =======
    ...new content...
    >>>>>>> END FILE src/main.rs >>>>>>>
    ```
  - 提供容错：若 `search` 为空则视为全文件替换；若文件不存在则创建新文件。
  - 对 `Ask` 模式不解析编辑标记，直接显示回复。

#### 4.7 工作区编辑应用

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\editor.rs`
- **改动**：新增 `apply_ai_workspace_edits(&mut self, edits: &[AiEdit]) -> Result<Vec<PathBuf>, String>`：
  - 对正在编辑的文件：定位到对应 `Tab`，使用 `buffer.delete` + `buffer.insert` 应用修改，记录 history，标记 dirty。
  - 对未打开的文件：从磁盘读取 -> 应用字符串替换 -> 写回磁盘；若不存在则创建父目录与文件。
  - 返回成功修改的文件路径列表。
  - 保留并复用现有 `apply_ai_code`（光标/选区插入）作为单点插入的快捷路径。

#### 4.8 Diff 预览视图

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\diff_view.rs`
- **内容**：
  - `DiffView` 结构体保存原始文本、建议文本、统一 diff 行列表。
  - 使用 `similar` crate（或自研基于行的 LCS）生成 diff。
  - 渲染方式 MVP 为统一 diff：删除行红色背景，新增行绿色背景，复用 `aether_render::TextRenderer`。
  - 提供 `accept(&mut EditorState)`、`reject(&mut self)`、`next_file()`/`prev_file()`。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_panel.rs`
- **改动**：在 AI 助手面板中增加“变更列表”区域：
  - 显示本次 Agent 编辑涉及的所有文件及行数统计。
  - 每个文件旁显示 “预览 / 接受 / 拒绝” 按钮。
  - 点击“预览”切换到底部或右侧的 `DiffView`。

#### 4.9 Agent 模式 UI

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_panel.rs`
- **改动**：
  - 在输入框上方增加模式切换：`Ask` / `Edit` / `Agent`。
  - `Edit` 模式：发送时附带编辑指令，返回后自动解析 `AiEdit` 并进入 Diff 预览。
  - `Agent` 模式（可选 MVP）：允许一次请求中包含“规划 + 编辑”；先按单轮实现，后续可扩展为多轮循环。

### Phase 3：AI 必需的编辑基础

没有这些，AI 生成的代码无法被验证、检索和修复。

#### 4.10 全局搜索

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-core\src\search.rs`
- **内容**：
  - `pub struct SearchQuery { pattern: String, regex: bool, case_sensitive: bool, include: Vec<String>, exclude: Vec<String> }`
  - `pub struct SearchResult { path: PathBuf, line: usize, col: usize, text: String }`
  - 实现 `search_workspace(query, root_dir) -> Vec<SearchResult>`：
    - 优先调用系统 `rg`（ripgrep），带回退到 `walkdir` + `regex`。
    - 限制单文件最大读取 1MB，总结果 500 条。
- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\search_panel.rs`
- **内容**：
  - 底部面板 UI：输入框、选项、结果列表。
  - 结果点击后在编辑器中打开对应文件并定位光标。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\command_palette.rs`
- **改动**：把 `Ctrl+Shift+F` 的“全局搜索”命令从占位改为真正打开搜索面板。

#### 4.11 LSP 诊断 UI

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\editor.rs`
- **改动**：
  - 新增字段 `diagnostics: HashMap<Url, Vec<lsp_types::Diagnostic>>`。
  - 在现有 LSP 事件处理中消费 `LspEvent::Diagnostics`（[client.rs:32](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-lsp/src/client.rs#L32)），更新 `diagnostics`。
  - 渲染时：在对应行下方绘制红色/黄色波浪线（可先用下划线简化），状态栏显示错误/警告数量。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_context.rs`
- **改动**：`AiContextAttachment::Diagnostics` 的 `to_prompt_fragment` 读取 `EditorState.diagnostics`，按 `severity` 排序，优先取当前文件。

#### 4.12 “AI 修复”快捷入口

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\editor.rs`
- **改动**：
  - 新增命令 `CommandId::AiFixDiagnostics`。
  - 当光标所在行存在诊断时，按 `Ctrl+.` 或点击灯泡图标，弹出菜单项“用 AI 修复”。
  - 执行时把当前文件、选区、诊断信息作为上下文发送到 `AiPanel`，并自动切换到 `Edit` 模式。

### Phase 4：设置与持久化

#### 4.13 会话持久化

- **新增文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_session.rs`
- **内容**：
  - `AiSession { id: String, title: String, created_at: DateTime<Utc>, messages: Vec<AiMessage>, attachments: Vec<AiContextAttachment> }`
  - 序列化到 `%APPDATA%/Aether/sessions/{id}.json`。
  - 加载最近 20 个会话列表。
- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\ai_panel.rs`
- **改动**：
  - 顶部增加会话标题与下拉列表（最近会话、新建会话）。
  - 每次发送消息后自动保存当前会话。

#### 4.14 命令面板补充

- **文件**：`d:\Application\牧羊人编辑器\crates\aether-win32\src\command_palette.rs`
- **改动**：新增命令项：
  - `AI: 新建会话`
  - `AI: 添加上下文`
  - `AI: 应用所有编辑`
  - `AI: 拒绝所有编辑`
  - `搜索: 全局搜索`
  - `查看: 切换 Diff 预览`

## 5. 关键设计决策

| 决策 | 选择 | 理由 |
|---|---|---|
| **流式实现** | 在 `aether-ai` 中用 `ureq` 读取 SSE，后台线程通过 `mpsc`/`Mutex` 推送 token | 不引入新异步运行时，复用现有同步代码路径 |
| **Agent 编辑协议** | 自定义文本标记 `<<<<<<< FILE ... >>>>>>>` | 不依赖 MCP/Tool 框架，先让端到端跑通；未来可迁移到标准协议 |
| **Diff 算法** | 使用 `similar` crate 或自研行级 LCS | 与现有 D2D 渲染配合简单，MVP 只提供统一 diff |
| **上下文组装位置** | 放在 `aether-win32::editor::EditorState` | EditorState 持有文件、选区、标签页、LSP 事件，是天然的上下文源 |
| **插件策略** | 本次不接入 WASM 运行时 | 按用户选择，先专注 AI 能力；`aether-plugin` 的 `PluginRuntime`（[runtime.rs:16](file:///d:/Application/%E7%89%A7%E7%BE%8A%E4%BA%BA%E7%BC%96%E8%BE%91%E5%99%A8/crates/aether-plugin/src/runtime.rs#L16)）保持占位 |
| **多文件编辑原子性** | 每个文件独立 history 记录；Agent 编辑应用后用户可逐文件撤销 | 避免一次性大撤销破坏用户体验 |

## 6. 验证步骤

### 6.1 单元测试

- `aether-ai`：SSE 流式解析测试（单 token、多行、空行、错误 json）。
- `ai_agent.rs`：编辑标记解析测试（正常替换、整文件替换、创建新文件、未闭合标记容错）。
- `ai_context.rs`：上下文组装测试（限制总长度、诊断排序）。
- `search.rs`：全局搜索测试（命中、排除、最大限制）。

### 6.2 集成/手动验证

1. 打开一个 Rust 工作区，启动 rust-analyzer。
2. 在 AI 面板输入“把当前函数改成返回 Result”，添加 `CurrentFile` 上下文，确认：
   - 响应是流式逐字出现；
   - 返回内容被解析为 `AiEdit`；
   - Diff 预览正确显示旧/新内容；
   - 点击“接受”后文件被修改且可撤销。
3. 故意写一段编译错误的代码，等待 LSP 诊断出现；执行“AI 修复”，确认诊断被作为上下文发送。
4. `Ctrl+Shift+F` 搜索项目中的某个字符串，确认结果可点击跳转。
5. 重启 Aether，确认最近 AI 会话可恢复。

### 6.3 不回归检查

- 原有 `apply_ai_code`（光标处插入）仍可用。
- 原有非流式 `chat_completion` 仍用于设置面板的“测试连接”。
- LSP 其他功能（补全、hover、格式化）不受诊断 UI 改动影响。

## 7. 文件改动总览

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `crates/aether-ai/src/lib.rs` | 修改 | 新增流式 SSE API |
| `crates/aether-shared/src/settings.rs` | 修改 | AI 参数扩展 |
| `crates/aether-win32/src/settings.rs` | 修改 | 设置 UI 增加字段 |
| `crates/aether-win32/src/ai_panel.rs` | 大幅修改 | 流式状态、附件 UI、Agent 模式、变更列表 |
| `crates/aether-win32/src/editor.rs` | 大幅修改 | 上下文收集、诊断存储、工作区编辑应用、AI 修复命令 |
| `crates/aether-win32/src/ai_context.rs` | 新增 | 上下文附件模型与 prompt 片段 |
| `crates/aether-win32/src/ai_prompt.rs` | 新增 | 提示词构建器 |
| `crates/aether-win32/src/ai_agent.rs` | 新增 | Agent 编辑解析 |
| `crates/aether-win32/src/diff_view.rs` | 新增 | Diff 预览视图 |
| `crates/aether-win32/src/ai_session.rs` | 新增 | 会话持久化 |
| `crates/aether-core/src/search.rs` | 新增 | 全局搜索逻辑 |
| `crates/aether-win32/src/search_panel.rs` | 新增 | 全局搜索面板 |
| `crates/aether-win32/src/command_palette.rs` | 修改 | 新增 AI/搜索命令 |
| `crates/aether-win32/src/events.rs` | 修改 | 新增 `AiPanelChanged`、`DiffViewChanged` 等事件 |
| `crates/aether-win32/src/render.rs` | 修改 | 轮询流式 token、渲染 Diff 视图、搜索面板 |
| `crates/aether-win32/src/layout.rs` | 修改 | 为 Diff/搜索面板增加区域计算 |

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 流式解析阻塞 UI | 后台线程读取 SSE，主线程每帧只取 `Mutex` 中的 `partial` |
| AI 生成错误代码被直接应用 | 所有 Agent 编辑先进入 Diff 预览，必须用户接受 |
| 上下文过长导致 token 爆炸 | 对每种附件设置硬上限，总上下文也设上限，超限自动省略 |
| 编辑标记语法与代码冲突 | 使用 `<<<<<<< FILE path >>>>>>>` 这种在日常代码中极低概率出现的标记；未来可迁移到 JSON/Tool 调用 |
| 多文件编辑后撤销困难 | 每个文件独立 history；同时提供“拒绝所有编辑”一键回滚 |

---

**计划完成。** 待你确认后，即可按 Phase 1 → Phase 4 顺序开始实现。
