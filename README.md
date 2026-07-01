# Aether Studio

Aether Studio 是一款基于 Rust + Win32 API 构建的现代化代码编辑器，面向 Windows 平台原生体验设计。它采用 Direct2D 自绘 UI，模块化 Crate 架构，致力于提供高性能、低延迟、可扩展的编辑与开发环境。

---

## 功能特性

- **原生 Windows 体验**：基于 Win32 窗口 + DWM Acrylic/Mica 背景效果，支持高 DPI、多显示器、DPI 缩放与系统高对比度模式
- **自绘渲染引擎**：基于 Direct2D / DirectWrite 的 2D 渲染管线，支持主题、玻璃效果、阴影、动画与脏矩形优化
- **文本编辑器核心**：Piece Table 数据结构、多光标、选择区、撤销/重做历史栈、语法高亮、查找替换、自动缩进
- **文件树与工作区**：异步文件扫描，默认折叠目录，支持文件打开、最近项目、拖拽与 Git 状态标记
- **侧边栏与面板**：文件树、Git 源码管理、AI 助手、终端、SSH 远程、设置面板、命令面板
- **AI 集成**：通过 HTTP 接口与大模型交互，支持代码解释、改写、应用建议
- **远程开发**：SSH 连接、远程文件树、Git 克隆与远程目录浏览
- **LSP 支持**：Language Server Protocol 客户端框架（持续完善中）
- **终端**：内置终端面板，支持 PowerShell/CMD 等子进程
- **插件系统**：预留插件扩展接口（aether-plugin）
- **国际化输入法**：支持 CJK IME 候选窗口定位与合成事件

---

## 项目架构

仓库采用 Cargo Workspace 组织，按职责拆分为多个 Crate：

| Crate | 说明 |
|---|---|
| `aether-core` | 编辑器核心：Piece Table 文本缓冲、历史栈、词法分析器、文件树数据结构 |
| `aether-render` | Direct2D / DirectWrite 渲染抽象、主题系统、画笔与文本格式缓存 |
| `aether-win32` | Windows 原生 UI 层：窗口、消息循环、菜单、布局、事件处理、应用入口 |
| `aether-shared` | 共享配置与持久化设置（UI 设置、最近项目、窗口状态） |
| `aether-lsp` | Language Server Protocol 客户端实现 |
| `aether-remote` | SSH / Git 远程操作抽象与协议实现 |
| `aether-ai` | AI 服务接口与请求处理 |
| `aether-plugin` | 插件扩展接口与运行时（预留） |
| `aether-tree-sitter` | Tree-sitter 语法解析集成（预留） |
| `aether-dap` | Debug Adapter Protocol 调试协议支持（预留） |

---

## 构建与运行

### 环境要求

- Windows 10 1809 或更高版本（推荐 Windows 11 以获得最佳 Mica/Acrylic 效果）
- Rust 1.70 或更高版本
- Visual Studio 2022（或已安装 Windows SDK 的构建工具）

### 构建

```powershell
# 编译调试版本
cargo build -p aether-win32 --bin aether-app

# 编译发布版本
cargo build -p aether-win32 --bin aether-app --release
```

### 运行

```powershell
cargo run -p aether-win32 --bin aether-app
```

编译产物位于：

```
target\x86_64-pc-windows-msvc\debug\aether-app.exe
```

---

## 开发指南

请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)，了解分支规范、提交前检查清单、CI 失败处理、合并冲突处理以及外部贡献者 Fork 流程。

快速参考：

```powershell
cargo fmt --all -- --check
cargo check -p aether-win32
cargo test --workspace --lib --no-fail-fast
```

---

## 常用快捷键

| 快捷键 | 功能 |
|---|---|
| `Ctrl + N` | 新建文件 |
| `Ctrl + O` | 打开文件 |
| `Ctrl + K` | 打开文件夹 |
| `Ctrl + S` | 保存 |
| `Ctrl + Shift + S` | 另存为 |
| `Ctrl + Z` | 撤销 |
| `Ctrl + Y` / `Ctrl + Shift + Z` | 重做 |
| `Ctrl + F` | 查找 |
| `Ctrl + H` | 替换 |
| `Ctrl + A` | 全选 |
| `Ctrl + Shift + P` | 命令面板 |
| `Ctrl + Shift + F` | 全局搜索 |
| `Ctrl + \`` | 切换终端 |
| `Ctrl + +` | 放大字体 |
| `Ctrl + -` | 缩小字体 |
| `F10` / `Alt` | 菜单栏键盘导航 |
| `Ctrl + 鼠标滚轮` | 横向滚动 |

---

## 设计原则

1. **性能优先**：UI 线程不阻塞，文件 IO 与远程操作异步化；渲染层使用缓存与脏矩形减少重绘
2. **原生体验**：Windows 原生窗口、系统菜单、输入法、DPI 感知、高对比度支持
3. **模块化**：通过 Workspace 与 Crate 拆分，核心逻辑与平台层解耦，便于测试与扩展
4. **可维护**：复杂函数拆分、借用检查合规、避免 unsafe 滥用、完善的单元测试

---

## 路线图

- [ ] 完善 LSP 语言服务器集成（补全、跳转、诊断）
- [ ] 扩展 Tree-sitter 语法解析支持
- [ ] 多窗口与多工作区支持
- [ ] 插件市场与运行时扩展
- [ ] 跨平台渲染抽象（远期）

---

## 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。

---

## 仓库

- GitHub：[https://github.com/songdiyang/AetherStudio](https://github.com/songdiyang/AetherStudio)
