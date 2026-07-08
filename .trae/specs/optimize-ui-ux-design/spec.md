# UI/UX 设计优化 Spec

## Why

当前 Aether 编辑器在视觉一致性与交互完整性上存在多个缺陷，影响用户对产品品质的感知：

1. **图标系统割裂**：活动栏用 emoji、标题栏按钮用像素点阵、其他区域用矢量 IconCache——三套并存的图标风格破坏整体感。
2. **标题栏工具按钮视觉粗糙**：返回/前进箭头由 7 个小方块拼成，关闭 X 由 20 个点拼成，用户头像由 12 个小矩形拼成，与 Direct2D 矢量绘制能力严重不匹配。
3. **反馈缺失**：活动栏 `tooltip` 字段定义了却从未渲染；标题栏工具按钮、状态栏分区、标签关闭按钮均无悬停提示；编辑器外区域鼠标光标不变化（无 IBEAM/HAND 切换）。
4. **交互不完整**：缺少中键关闭标签、标签拖拽重排、标签右键菜单、Alt+Left/Right 导航、Ctrl+, 打开设置、Ctrl+J 切换底部面板等行业标准操作。
5. **视觉细节欠打磨**：状态栏高度 22px 偏窄、活动栏 inactive 图标对比度不足、标签栏缺少"新建标签"按钮、空状态（无最近项目、无终端、无 Git）缺乏引导设计。

本规范聚焦**视觉美学与操作体验**，不重复 `aether-ux-prd.md` 中已覆盖的功能性 Bug（光标索引、撤销历史、脏矩形等）。

## What Changes

### 视觉一致性
- 统一图标系统：活动栏、标题栏按钮、欢迎页 logo 全部迁移到 `IconCache` 矢量绘制
- 新增矢量图标：`Back`/`Forward`/`Settings`/`User`/`ChevronLeft`/`ChevronRight`/`Plus`/`Close`/`EmojiSheep`（欢迎页 logo）
- 移除 `ActivityBarView::icon()` 返回 emoji 的实现，改为返回 `IconKind`
- 标题栏按钮（返回/前进/关闭/最小化/最大化/设置/用户）全部改用矢量 PathGeometry 绘制

### 视觉打磨
- 活动栏 inactive 图标颜色提亮（从 0.5 提升到 0.55），改善对比度
- 状态栏增加 hover 背景高亮（clickable 分区）
- 标签栏新增"新建标签"按钮（+ 图标，位于标签栏右侧）
- 标签关闭按钮改用矢量 X 图标，替换 "×" 字符
- 标签 dirty 状态用独立小圆点图标显示，不混入文件名文本
- 欢迎页空状态（无最近项目）改为带图标 + 引导文案
- 欢迎页 logo 改用矢量 `EmojiSheep` 图标（保留羊形象，但用 PathGeometry 绘制）

### 反馈与提示
- 实现 tooltip 渲染系统：活动栏、标题栏工具按钮、状态栏分区、标签关闭按钮均显示悬停提示
- 鼠标光标语义化：
  - 编辑器内容区 → `IDC_IBEAM`
  - 可点击元素（活动栏、标签、工具按钮、菜单项）→ `IDC_HAND`
  - 文本输入区（搜索框、命令面板）→ `IDC_IBEAM`
  - 不可点击区域 → `IDC_ARROW`
- 标签、活动栏、菜单项 hover 时增加细微过渡（颜色渐变，150ms 内）

### 交互完整性
- 标签页交互：
  - 中键点击关闭标签
  - 标签拖拽重排（同方向）
  - 标签右键菜单（关闭、关闭其他、关闭右侧、关闭所有、复制路径、在文件资源管理器中打开）
  - 标签栏鼠标滚轮横向滚动
- 键盘快捷键补全：
  - `Ctrl+,` 打开设置
  - `Ctrl+J` 切换底部面板
  - `Ctrl+Shift+T` 恢复最后关闭的标签
  - `Alt+Left` / `Alt+Right` 导航返回/前进
  - `Ctrl+Shift+E` 切换资源管理器视图
  - `Ctrl+Shift+G` 切换源代码管理视图
- 活动栏右键菜单（隐藏、自定义排序）
- 面板最小尺寸限制（侧边栏 ≥ 150px、底部面板 ≥ 100px、右侧面板 ≥ 150px）

### 空状态与引导
- 欢迎页"暂无最近项目"改为：文件夹图标 + "尚未打开过项目" + "打开文件夹开始" 按钮
- 终端未启动时显示提示文案
- Git 非 repo 时状态栏 Git 分支区域隐藏（而非显示空标签）

## Impact

- **Affected specs**: 无（首个 spec）
- **Affected code**:
  - `crates/aether-win32/src/icons.rs` — 新增 9 个 IconKind 变体及对应几何
  - `crates/aether-win32/src/layout.rs` — `ActivityBarView::icon()` 返回类型变更
  - `crates/aether-win32/src/activity_bar.rs` — 移除 emoji 依赖
  - `crates/aether-win32/src/render.rs` — 标题栏按钮、活动栏、标签栏、状态栏渲染重写
  - `crates/aether-win32/src/welcome.rs` — logo 与空状态改写
  - `crates/aether-win32/src/window/mouse_handler/mouse_move.rs` — tooltip 显示与光标切换
  - `crates/aether-win32/src/window/mouse_handler/l_button_down.rs` — 中键关闭标签、标签右键菜单
  - `crates/aether-win32/src/window/keyboard_handler/key_down_ctrl.rs` — 新增快捷键
  - `crates/aether-win32/src/tabs.rs` — 新建标签按钮、拖拽重排
  - `crates/aether-win32/src/status_bar.rs` — hover 状态、分区显隐
  - 新增 `crates/aether-win32/src/tooltip.rs` — tooltip 渲染与状态管理
  - 新增 `crates/aether-win32/src/tab_context_menu.rs` — 标签右键菜单

## ADDED Requirements

### Requirement: 统一矢量图标系统

系统 SHALL 使用 `IconCache` 矢量 PathGeometry 绘制所有 UI 图标，不再使用 emoji 或像素点阵。

#### Scenario: 活动栏图标渲染
- **WHEN** 应用渲染活动栏
- **THEN** 每个视图项使用 `IconKind` 对应的矢量图标绘制
- **AND** 不调用 `DrawText` 渲染 emoji 字符
- **AND** 图标在 24x24 视口内保持比例居中

#### Scenario: 标题栏按钮渲染
- **WHEN** 应用渲染标题栏工具按钮（返回/前进/侧边栏/面板/设置/用户/最小化/最大化/关闭）
- **THEN** 每个图标使用 `IconKind` 矢量几何绘制
- **AND** 不使用 `FillRectangle` 拼接像素点
- **AND** 线条笔画宽度为 1.5（视口单位），与现有图标一致

#### Scenario: 欢迎页 logo
- **WHEN** 欢迎页渲染品牌 logo
- **THEN** 使用矢量 `EmojiSheep` 图标（PathGeometry 绘制的羊形）
- **AND** 不使用 emoji 字符 "🐑"

### Requirement: Tooltip 悬停提示系统

系统 SHALL 在用户悬停可交互元素 500ms 后显示文字提示，移动鼠标后立即消失。

#### Scenario: 活动栏 tooltip
- **WHEN** 用户将鼠标悬停在活动栏某项上 500ms
- **THEN** 在该项右侧显示包含 `tooltip` 文本的提示框
- **AND** 鼠标移开后提示框消失

#### Scenario: 标题栏工具按钮 tooltip
- **WHEN** 用户将鼠标悬停在标题栏工具按钮上 500ms
- **THEN** 显示按钮功能说明（如 "切换侧边栏"、"设置"、"用户菜单"）
- **AND** 鼠标移开后提示框消失

#### Scenario: 状态栏分区 tooltip
- **WHEN** 用户将鼠标悬停在状态栏 clickable 分区上 500ms
- **THEN** 显示该分区功能说明（如 "Git 分支"、"编码"、"语言模式"）
- **AND** 鼠标移开后提示框消失

### Requirement: 鼠标光标语义化

系统 SHALL 根据鼠标所在区域切换系统光标，提供视觉反馈。

#### Scenario: 编辑器文本区
- **WHEN** 鼠标位于编辑器内容区域
- **THEN** 光标变为 `IDC_IBEAM`

#### Scenario: 可点击元素
- **WHEN** 鼠标位于活动栏、标签栏、菜单栏、工具按钮、状态栏 clickable 分区
- **THEN** 光标变为 `IDC_HAND`

#### Scenario: 不可点击区域
- **WHEN** 鼠标位于编辑器非内容区、面板边框、空白区域
- **THEN** 光标保持 `IDC_ARROW`

### Requirement: 标签页交互完整性

系统 SHALL 提供完整的标签页操作能力，与主流编辑器一致。

#### Scenario: 中键关闭标签
- **WHEN** 用户中键点击标签
- **THEN** 该标签关闭
- **AND** 若关闭的是活动标签，则切换到相邻标签

#### Scenario: 标签拖拽重排
- **WHEN** 用户左键按住标签并拖动
- **THEN** 标签跟随鼠标移动
- **AND** 释放时标签插入到目标位置
- **AND** 拖拽中显示插入位置指示线

#### Scenario: 标签右键菜单
- **WHEN** 用户右键点击标签
- **THEN** 显示上下文菜单，包含：关闭、关闭其他、关闭右侧、关闭所有、复制路径、在文件资源管理器中打开
- **AND** 每个菜单项触发对应操作

#### Scenario: 标签栏横向滚动
- **WHEN** 标签数量超出可视宽度且用户在标签栏滚动鼠标滚轮
- **THEN** 标签栏横向滚动
- **AND** 滚动方向跟随滚轮方向

#### Scenario: 新建标签按钮
- **WHEN** 标签栏可见
- **THEN** 标签栏右侧显示 "+" 新建标签按钮
- **AND** 点击后创建空白标签

### Requirement: 键盘快捷键补全

系统 SHALL 支持以下行业标准快捷键。

#### Scenario: 设置快捷键
- **WHEN** 用户按下 `Ctrl+,`
- **THEN** 打开设置侧边栏

#### Scenario: 底部面板快捷键
- **WHEN** 用户按下 `Ctrl+J`
- **THEN** 切换底部面板可见性

#### Scenario: 恢复关闭标签
- **WHEN** 用户按下 `Ctrl+Shift+T`
- **THEN** 恢复最后关闭的标签（若有历史记录）

#### Scenario: 导航快捷键
- **WHEN** 用户按下 `Alt+Left` 或 `Alt+Right`
- **THEN** 触发返回/前进导航

#### Scenario: 视图切换快捷键
- **WHEN** 用户按下 `Ctrl+Shift+E`
- **THEN** 切换到资源管理器视图
- **WHEN** 用户按下 `Ctrl+Shift+G`
- **THEN** 切换到源代码管理视图

### Requirement: 状态栏交互反馈

状态栏 SHALL 提供清晰的 hover 与点击反馈。

#### Scenario: Hover 高亮
- **WHEN** 鼠标悬停在状态栏 clickable 分区
- **THEN** 该分区背景变亮
- **AND** 显示 tooltip 说明

#### Scenario: Git 非 repo 隐藏
- **WHEN** 当前工作区不是 Git 仓库
- **THEN** 状态栏 Git 分支分区隐藏（不显示空标签）
- **AND** 其他分区右移填补空间

### Requirement: 空状态引导设计

系统 SHALL 为空状态提供带图标的引导界面，而非纯文本。

#### Scenario: 欢迎页无最近项目
- **WHEN** 欢迎页渲染且无最近项目
- **THEN** 显示文件夹图标 + "尚未打开过项目" 文案 + "打开文件夹" 按钮
- **AND** 点击按钮触发打开文件夹对话框

#### Scenario: 终端未启动
- **WHEN** 终端面板可见但未启动
- **THEN** 显示提示文案 "按 Ctrl+` 启动终端"

### Requirement: 面板最小尺寸

系统 SHALL 强制面板最小尺寸，防止用户误操作导致 UI 不可用。

#### Scenario: 侧边栏最小宽度
- **WHEN** 用户拖拽侧边栏分隔条
- **THEN** 侧边栏宽度不低于 150px

#### Scenario: 底部面板最小高度
- **WHEN** 用户拖拽底部面板分隔条
- **THEN** 底部面板高度不低于 100px

#### Scenario: 右侧面板最小宽度
- **WHEN** 用户拖拽右侧面板分隔条
- **THEN** 右侧面板宽度不低于 150px

## MODIFIED Requirements

### Requirement: 活动栏图标

活动栏 `ActivityBarView` 的图标改为返回 `IconKind`，不再返回 emoji 字符串。

```rust
// 修改前
pub fn icon(&self) -> &'static str {
    match self {
        ActivityBarView::Explorer => "📁",
        // ...
    }
}

// 修改后
pub fn icon(&self) -> crate::icons::IconKind {
    match self {
        ActivityBarView::Explorer => crate::icons::IconKind::Folder,
        ActivityBarView::SourceControl => crate::icons::IconKind::GitBranch,
        ActivityBarView::Terminal => crate::icons::IconKind::Terminal,
        ActivityBarView::RemoteManager => crate::icons::IconKind::Ssh,
        ActivityBarView::AiAssistant => crate::icons::IconKind::Bug, // 或新增 Bot 图标
    }
}
```

### Requirement: 标签 dirty 状态显示

标签 dirty 指示器从文件名文本中分离，作为独立小圆点图标显示在文件名右侧。

#### Scenario: Dirty 标签显示
- **WHEN** 标签内容已修改未保存
- **THEN** 文件名右侧显示小圆点图标（直径 6px）
- **AND** 文件名文本不包含 "●" 字符

### Requirement: 状态栏分区宽度

状态栏分区宽度根据内容自适应，不再硬编码。

#### Scenario: 自适应宽度
- **WHEN** 状态栏渲染
- **THEN** 每个分区宽度根据文本测量 + padding 计算
- **AND** 最小宽度为 40px
- **AND** 最大宽度为 200px

## REMOVED Requirements

### Requirement: emoji 图标使用

**Reason**: emoji 渲染依赖系统字体，不同 Windows 版本显示不一致；与矢量图标风格冲突。
**Migration**: 所有 emoji 图标替换为 `IconKind` 矢量图标，通过 `IconCache::draw` 渲染。
