# Aether Studio（牧羊人编辑器）

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows-lightgrey.svg)]()

## The Repository / 仓库说明

This repository is where we develop the **Aether Studio** product. Aether Studio is a modern code editor built with Rust and Win32 API, designed for native Windows experience with high performance and low latency. The source code is available to everyone under the standard [MIT license](LICENSE).

本仓库是 **Aether Studio（牧羊人编辑器）** 的开发仓库。Aether Studio 是一款基于 Rust + Win32 API 构建的现代化代码编辑器，面向 Windows 平台原生体验设计，追求高性能与低延迟。源代码在标准 [MIT 许可证](LICENSE) 下向所有人开放。

## Aether Studio / 产品简介

Aether Studio combines the simplicity of a lightweight editor with what developers need for their core edit-build-debug cycle. It provides comprehensive code editing, navigation, and syntax highlighting support along with AI-assisted coding, a modular extensibility model, and native Windows integration.

Aether Studio 将轻量级编辑器的简洁性与开发者核心编辑-构建-调试周期所需的功能相结合。提供全面的代码编辑、导航、语法高亮支持，以及 AI 辅助编程、模块化可扩展模型和原生 Windows 集成。

Aether Studio is actively developed with new features and bug fixes. It is currently available for **Windows 10 1809+** and **Windows 11**.

Aether Studio 正在积极开发中，持续推出新功能和错误修复。目前支持 **Windows 10 1809+** 和 **Windows 11**。

## Features / 功能特性

- **Native Windows Experience / 原生 Windows 体验**: Win32 window with DWM immersive dark mode, high DPI support, DPI scaling, and system high contrast mode. 基于 Win32 窗口 + DWM 沉浸式深色模式，支持高 DPI、DPI 缩放与系统高对比度模式。
- **Self-Rendered UI Engine / 自绘渲染引擎**: Direct2D / DirectWrite 2D rendering pipeline with themes, translucent backgrounds, shadows, animations, and dirty rectangle optimization. 基于 Direct2D / DirectWrite 的 2D 渲染管线，支持主题、半透明背景、阴影、动画与脏矩形优化。
- **High-Performance Text Editing / 高性能文本编辑**: Piece Table text buffer, multi-cursor support, selection, undo/redo history stack, syntax highlighting, find/replace, and auto-indentation. Piece Table 文本缓冲、多光标、选择区、撤销/重做历史栈、语法高亮、查找替换、自动缩进。
- **Multi-Language Lexer / 多语言词法分析器**: Built-in tokenization for C, Rust, JavaScript, Python, JSON, TOML, HTML, Markdown, and more. 内置 C、Rust、JavaScript、Python、JSON、TOML、HTML、Markdown 等语言的分词支持。
- **File Tree & Workspace / 文件树与工作区**: Asynchronous file scanning, Git status markers, recent projects, and drag-and-drop support. 异步文件扫描、Git 状态标记、最近项目、拖拽支持。
- **AI Integration / AI 集成**: HTTP-based LLM interaction with DeepSeek and Kimi presets, code explanation, rewrite, inline suggestions, and API configuration panel. 基于 HTTP 的大模型交互，支持 DeepSeek / Kimi 预设配置、代码解释、改写、内联建议与 API 配置面板。
- **LSP Support / LSP 支持**: Language Server Protocol client framework (document sync, semantic tokens, incremental sync). 语言服务器协议客户端框架（文档同步、语义 token、增量同步）。
- **DAP Support / DAP 支持**: Debug Adapter Protocol client foundation (types, transport, session, client). 调试适配器协议客户端基础实现。
- **Tree-sitter Integration / Tree-sitter 集成**: Syntax parsing, language detection, TextMate theme mapping, and highlight rendering. 语法解析、语言检测、TextMate 主题映射与高亮渲染。
- **Remote Development / 远程开发**: SSH connection, remote file system, Git clone, and remote directory browsing. SSH 连接、远程文件系统、Git 克隆与远程目录浏览。
- **Terminal Panel / 终端面板**: Built-in terminal panel with PowerShell / CMD subprocess support. 内置终端面板，支持 PowerShell / CMD 子进程。
- **Plugin System / 插件系统**: Plugin registration, permission control, and runtime foundation. 插件注册、权限控制与运行时基础。
- **International Input Method / 国际化输入法**: CJK IME candidate window positioning and composition events. CJK 输入法候选窗口定位与合成事件。
- **Command Line Tool / 命令行工具**: `aether` CLI to launch GUI, open paths, navigate to line/column, and support `--wait` / `--new-window`. `aether` CLI 可启动 GUI、打开路径、定位行列并支持 `--wait` / `--new-window`。

## Contributing / 参与贡献

There are many ways in which you can participate in this project:

您可以通过多种方式参与本项目：

* [Submit bugs and feature requests / 提交错误和功能请求](https://github.com/songdiyang/AetherStudio/issues)
* [Review source code changes / 审查源代码变更](https://github.com/songdiyang/AetherStudio/pulls)
* [Review the documentation / 审查文档](.qoder/repowiki) and make pull requests for anything from typos to new content. 并针对从拼写错误到新内容的任何内容提交 PR。

If you are interested in fixing issues and contributing directly to the code base, please see the document [CONTRIBUTING.md](CONTRIBUTING.md), which covers the following:

如果您有兴趣修复问题并直接贡献代码，请参阅 [CONTRIBUTING.md](CONTRIBUTING.md)，其中涵盖以下内容：

* [How to build and run from source / 如何从源码构建和运行](CONTRIBUTING.md)
* [The development workflow, including testing and debugging / 开发工作流，包括测试和调试](CONTRIBUTING.md)
* [Coding guidelines / 编码规范](CONTRIBUTING.md)
* [Submitting pull requests / 提交 PR](CONTRIBUTING.md)

## Building from Source / 从源码构建

### Requirements / 环境要求

- Windows 10 1809 or higher (Windows 11 recommended) / Windows 10 1809 或更高版本（推荐 Windows 11）
- Rust 1.70 or higher / Rust 1.70 或更高版本
- Visual Studio 2022 (or Windows SDK build tools) / Visual Studio 2022（或 Windows SDK 构建工具）

### Build / 构建

```powershell
# Debug build / 调试构建
cargo build -p aether-win32 --bin aether-app

# Release build (fat LTO, single codegen unit, strip) / 发布构建
cargo build -p aether-win32 --bin aether-app --release

# CLI tool / 命令行工具
cargo build -p aether-cli --bin aether
```

### Run / 运行

```powershell
# Launch GUI directly / 直接启动 GUI
cargo run -p aether-win32 --bin aether-app

# Open file via CLI / 通过 CLI 打开文件
cargo run -p aether-cli --bin aether -- path/to/file.rs

# Navigate to line:column / 定位到指定行列
cargo run -p aether-cli --bin aether -- file.txt:10:5
```

Build artifacts are located at / 编译产物位于：

```
target\x86_64-pc-windows-msvc\debug\aether-app.exe
target\x86_64-pc-windows-msvc\release\aether-app.exe
```

## Testing / 测试

```powershell
# Full workspace unit tests (disable incremental compilation to avoid ICE) / 全 Workspace 单元测试
$env:CARGO_INCREMENTAL = '0'
cargo test --workspace --no-fail-fast

# Static analysis / 静态分析
cargo clippy --workspace --all-targets -- -D warnings

# Format check / 格式化检查
cargo fmt --all -- --check

# GUI smoke test (requires release build) / GUI 冒烟测试
cargo build --release -p aether-win32
python tests/gui_smoke.py
```

Latest test snapshot (2026-07-06) / 最新测试快照：

| Metric / 指标 | Result / 结果 |
|---|---|
| Unit tests / 单元测试 | **793 tests passing / 793 个测试全部通过** |
| Code coverage / 代码覆盖率 | Regions 47.82% / Lines 43.70% / Functions 61.66% |
| Clippy | Passing with `-D warnings` / 通过 |
| Release build / 发布构建 | Successful / 成功 |
| GUI smoke test / GUI 冒烟测试 | Successful startup, click, screenshot, close / 成功启动、点击、截图、关闭 |
| Memory / 内存 | ~67MB |
| Lexer benchmark / 词法分析器基准 | ~500–650 MiB/s |

> Coverage is affected by Win32 / Direct2D GUI rendering code, window procedures, real subprocesses, and network interactions. Business logic modules that can be unit-tested generally reach **80–100%** coverage.
>
> 覆盖率受大量 Win32 / Direct2D GUI 渲染代码、窗口过程、真实子进程与网络交互代码拖累。可单元测试的业务逻辑模块覆盖率普遍达到 **80–100%**。

## Project Architecture / 项目架构

The repository is organized as a Cargo Workspace with multiple crates / 仓库采用 Cargo Workspace 组织，按职责拆分为多个 Crate：

| Crate | Responsibility / 职责 |
|---|---|
| `aether-core` | Text buffer, history stack, lexer, workspace data structures, search / 编辑器核心：文本缓冲、历史栈、词法分析器、工作区数据结构、搜索 |
| `aether-render` | Direct2D / DirectWrite rendering abstraction, theme system, brush cache / 渲染抽象、主题系统、画笔缓存 |
| `aether-win32` | Windows native UI layer: window, message loop, menus, layout, events, app entry / Windows 原生 UI 层：窗口、消息循环、菜单、布局、事件处理、应用入口 |
| `aether-shared` | Shared configuration and persistence (UI, AI, recent projects, window state) / 共享配置与持久化设置 |
| `aether-lsp` | Language Server Protocol client (sync, incremental sync, semantic tokens) / LSP 客户端实现 |
| `aether-dap` | Debug Adapter Protocol client foundation / DAP 客户端基础实现 |
| `aether-remote` | SSH / Git / container remote operation abstraction / 远程操作抽象 |
| `aether-ai` | AI service interface and request handling / AI 服务接口与请求处理 |
| `aether-tree-sitter` | Tree-sitter syntax parsing, language detection, theme mapping / 语法解析、语言检测、主题映射 |
| `aether-plugin` | Plugin registration, permissions, and runtime / 插件注册、权限与运行时 |
| `aether-cli` | Command-line launcher, parsing arguments and launching the GUI / 命令行启动器 |

## Documentation / 文档

The project includes a comprehensive internal documentation system under `.qoder/repowiki/` covering:

项目内置详细的内部文档系统（位于 `.qoder/repowiki/`），涵盖：

- **Project Overview / 项目概述**: Architecture, core components, dependencies, performance considerations / 架构总览、核心组件、依赖关系、性能考量
- **API Reference / API 参考**: Public interfaces, CLI, plugin development interfaces, configuration / 公共接口、命令行接口、插件开发接口、配置接口
- **UI System / UI 系统**: Theme system, rendering pipeline, window management, input events / 主题系统、渲染管线、窗口管理、输入事件处理
- **Architecture Design / 架构设计**: Overall architecture, text buffer system, lexer framework, rendering engine / 整体架构、文本缓冲区系统、词法分析器框架、渲染引擎
- **Core Components / 核心组件**: Workspace management, search system, text buffer, rendering engine / 工作区管理、搜索系统、文本缓冲区、渲染引擎
- **Performance Optimization / 性能优化**: SIMD algorithms, memory management, async I/O, rendering optimization / SIMD 算法、内存管理、异步 IO、渲染优化
- **Extension System / 扩展系统**: AI assistant integration, plugin architecture, remote development / AI 助手集成、插件架构、远程开发
- **Language Support / 语言支持**: LSP client, DAP debugging, Tree-sitter integration / LSP 客户端、DAP 调试、Tree-sitter 集成

## Feedback / 反馈

* [Request a new feature / 请求新功能](https://github.com/songdiyang/AetherStudio/issues)
* [File an issue / 提交问题](https://github.com/songdiyang/AetherStudio/issues)
* Follow the project and let us know what you think! / 关注项目并告诉我们您的想法！

## License / 许可证

Copyright (c) Aether Studio Contributors.

Licensed under the [MIT](LICENSE) license.

本项目采用 [MIT 许可证](LICENSE)。详见 LICENSE 文件。

## Repository / 仓库

- GitHub: [https://github.com/songdiyang/AetherStudio](https://github.com/songdiyang/AetherStudio)
