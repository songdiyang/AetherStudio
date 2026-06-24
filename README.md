# Aether Editor

Aether 是一款基于 Rust 构建的高性能代码编辑器，专为 Windows 平台设计，追求极致的渲染性能与流畅的用户体验。

## 特性

- **Rust 驱动**：充分利用 Rust 的内存安全与并发性能，打造稳定可靠的编辑器核心
- **高性能渲染**：基于 Direct2D 的渲染管线，目标 60fps 无掉帧，输入延迟 < 16ms
- **毛玻璃 UI**：Windows 10/11 原生 Acrylic/Glass 视觉效果，层次清晰的现代界面
- **多语言支持**：内置 C、Rust、Python、JavaScript、HTML、JSON、Markdown、TOML 等语法高亮
- **Git 集成**：完整的仓库管理、分支操作、提交与状态显示
- **SSH 远程开发**：支持远程文件系统浏览、编辑与终端会话
- **AI 辅助**：可配置 OpenAI、Claude、Kimi 等大模型 API，提供智能代码补全
- **LSP/DAP 支持**：内置语言服务器协议与调试适配器协议客户端
- **插件系统**：可扩展的插件架构，支持自定义功能扩展

## 技术栈

- **语言**：Rust (Edition 2021)
- **渲染**：Direct2D 1.1 + Win32 API
- **架构**：Workspace 多 Crate 模块化设计
- **平台**：Windows 10/11

## 项目结构

```
crates/
├── aether-core      # 编辑器核心：文本缓冲区、词法分析、工作区管理
├── aether-render    # Direct2D 渲染引擎与主题系统
├── aether-win32     # Win32 窗口、UI 组件、输入处理（主入口）
├── aether-lsp       # LSP 客户端实现
├── aether-dap       # DAP 调试客户端
├── aether-tree-sitter # 语法高亮引擎
├── aether-plugin    # 插件系统与运行时
├── aether-remote    # Git/SSH/容器远程开发支持
├── aether-ai        # AI 配置与 API 调用
├── aether-shared    # 跨 crate 共享配置与工具
└── aether-terminal  # 终端内嵌支持
```

## 快速开始

### 环境要求

- Windows 10/11
- Rust 工具链 (>= 1.70)

### 构建

```bash
cargo build --release
```

### 运行

```bash
cargo run --bin aether-win32
```

## 开发状态

Aether 目前处于活跃开发阶段，核心编辑功能已可用，正在持续完善 UI 体验与远程开发能力。

## 许可证

MIT License
