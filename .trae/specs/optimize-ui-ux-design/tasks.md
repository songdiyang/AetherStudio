# Tasks

## 阶段一：视觉一致性（图标系统统一）

- [x] Task 1: 新增矢量图标几何
  - [ ] SubTask 1.1: 在 `icons.rs` 的 `IconKind` 枚举中新增变体：`Back`、`Forward`、`Settings`、`User`、`Close`、`Plus`、`ChevronLeft`、`ChevronRight`、`EmojiSheep`、`Bot`（AI 助手用）
  - [ ] SubTask 1.2: 在 `build_icon` 中为每个新变体实现 PathGeometry 几何（参考 Lucide 图标风格，24x24 视口，1.5 笔画宽度）
  - [ ] SubTask 1.3: 更新 `IconKind::ALL` 常量数组包含所有新变体
  - [ ] SubTask 1.4: 编写单元测试验证每个新图标的几何非空

- [x] Task 2: 活动栏迁移到矢量图标
  - [ ] SubTask 2.1: 修改 `layout.rs` 中 `ActivityBarView::icon()` 返回类型为 `crate::icons::IconKind`
  - [ ] SubTask 2.2: 更新 `activity_bar.rs` 中 `ActivityItem` 不再依赖 emoji 字符串
  - [ ] SubTask 2.3: 重写 `render.rs::render_activity_bar` 使用 `IconCache::draw` 绘制图标，移除 `DrawText` emoji 路径
  - [ ] SubTask 2.4: 调整 inactive 图标颜色从 `0.5` 提升到 `0.55`，active 保持白色
  - [ ] SubTask 2.5: 验证活动栏渲染正确，所有视图显示对应矢量图标

- [x] Task 3: 标题栏按钮迁移到矢量图标
  - [ ] SubTask 3.1: 重写 `render_title_bar` 中返回/前进按钮，使用 `IconKind::Back`/`Forward`
  - [ ] SubTask 3.2: 重写关闭按钮，使用 `IconKind::Close` 替换 20 个像素点
  - [ ] SubTask 3.3: 重写设置按钮，使用 `IconKind::Settings` 替换手绘齿轮
  - [ ] SubTask 3.4: 重写用户按钮，使用 `IconKind::User` 替换 12 个像素点
  - [ ] SubTask 3.5: 最小化/最大化按钮保持现有几何（已足够清晰），仅整理代码
  - [ ] SubTask 3.6: 验证标题栏所有按钮 hover 状态与图标对齐

- [x] Task 4: 欢迎页 logo 迁移
  - [ ] SubTask 4.1: 重写 `welcome.rs::render_welcome_page` 中 logo 渲染，使用 `IconKind::EmojiSheep` 替换 emoji "🐑"
  - [ ] SubTask 4.2: 调整 logo 尺寸为 60x60，居中于左侧品牌区
  - [ ] SubTask 4.3: 移除 `logo_format` 文本格式相关代码
  - [ ] SubTask 4.4: 验证欢迎页 logo 显示正常

## 阶段二：反馈与提示系统

- [x] Task 5: Tooltip 渲染系统
  - [ ] SubTask 5.1: 新建 `crates/aether-win32/src/tooltip.rs` 模块，定义 `TooltipState` 结构（内容、位置、计时器）
  - [ ] SubTask 5.2: 实现 tooltip 渲染函数：半透明背景 + 圆角 + 文本，使用 Direct2D DrawText
  - [ ] SubTask 5.3: 在 `EditorState` 中新增 `tooltip_state: TooltipState` 字段
  - [ ] SubTask 5.4: 在 `mouse_move.rs` 中集成 tooltip 触发逻辑：检测 hover 元素，500ms 后显示
  - [ ] SubTask 5.5: 鼠标移动超过 4px 容差或离开元素时重置计时器并隐藏 tooltip
  - [ ] SubTask 5.6: 在 `render.rs` 主渲染流程末尾调用 tooltip 渲染（最上层）

- [x] Task 6: 鼠标光标语义化
  - [ ] SubTask 6.1: 在 `mouse_move.rs` 中根据当前 hover 区域返回光标类型枚举（`Arrow`/`IBeam`/`Hand`/`SizeWE`/`SizeNS`）
  - [ ] SubTask 6.2: 编辑器内容区返回 `IDC_IBEAM`
  - [ ] SubTask 6.3: 活动栏、标签栏、菜单栏、工具按钮、状态栏 clickable 分区返回 `IDC_HAND`
  - [ ] SubTask 6.4: 不可点击区域返回 `IDC_ARROW`
  - [ ] SubTask 6.5: 在 `WM_SETCURSOR` 消息处理中调用 `SetCursor`（新增消息处理）

## 阶段三：标签页交互完整性

- [x] Task 7: 标签页基础交互
  - [ ] SubTask 7.1: 在 `l_button_down.rs` 中添加中键点击检测，命中标签时调用 `close_tab(index)`
  - [ ] SubTask 7.2: 标签栏右侧添加 "+" 新建标签按钮，点击调用 `new_tab()`
  - [ ] SubTask 7.3: 标签关闭按钮改用 `IconKind::Close` 矢量图标，替换 "×" 字符
  - [ ] SubTask 7.4: 标签 dirty 状态用独立小圆点图标显示在文件名右侧，移除文件名中的 "●" 字符
  - [ ] SubTask 7.5: 标签栏鼠标滚轮横向滚动（`WM_MOUSEWHEEL` 转换为 `tab_scroll_x` 增量）

- [x] Task 8: 标签拖拽重排
  - [ ] SubTask 8.1: 在 `tabs.rs` 的 `Tab` 或 `EditorState` 中新增 `dragging_tab: Option<usize>` 字段
  - [ ] SubTask 8.2: 在 `l_button_down.rs` 中标签命中且按下后超过 3px 移动时进入拖拽模式
  - [ ] SubTask 8.3: 在 `mouse_move.rs` 中更新拖拽位置，计算 drop_index 并显示插入指示线
  - [ ] SubTask 8.4: 在 `l_button_up.rs`（新增处理）中执行 `tabs.remove(drag) + tabs.insert(drop, tab)` 重排
  - [ ] SubTask 8.5: 调整 `active_tab` 索引跟随移动

- [x] Task 9: 标签右键菜单
  - [ ] SubTask 9.1: 新建 `crates/aether-win32/src/tab_context_menu.rs` 模块，参考 `context_menu.rs` 结构
  - [ ] SubTask 9.2: 定义菜单项：关闭、关闭其他、关闭右侧、关闭所有、分隔符、复制路径、在文件资源管理器中打开
  - [ ] SubTask 9.3: 在 `r_button_down.rs` 中标签命中时打开 tab_context_menu
  - [ ] SubTask 9.4: 实现每个菜单项的点击处理逻辑
  - [ ] SubTask 9.5: 实现菜单渲染（复用 `context_menu.rs` 的 Direct2D 自绘方案）

## 阶段四：视觉打磨

- [x] Task 10: 状态栏优化
  - [ ] SubTask 10.1: 状态栏 clickable 分区添加 hover 背景高亮（亮色半透明矩形）
  - [ ] SubTask 10.2: Git 非 repo 时隐藏 Git 分支分区，其他右侧分区右移
  - [ ] SubTask 10.3: 分区宽度改为根据文本测量自适应（`IDWriteTextLayout::GetMetrics`），最小 40px、最大 200px
  - [ ] SubTask 10.4: 移除硬编码 `width: 120.0` 等魔法数字

- [x] Task 11: 欢迎页空状态
  - [ ] SubTask 11.1: 重写 "暂无最近项目" 渲染：`IconKind::Folder` 大图标（48x48）+ 主文案 + 副文案
  - [ ] SubTask 11.2: 添加 "打开文件夹" 按钮，点击触发 `WelcomeAction::OpenFolder`
  - [ ] SubTask 11.3: 移除纯文本 "暂无最近项目" 渲染代码

- [x] Task 12: 终端未启动提示
  - [ ] SubTask 12.1: 在 `render_bottom_panel` 中检测 `terminal_panel.running == false`
  - [ ] SubTask 12.2: 显示居中提示文案 "按 Ctrl+` 启动终端"（灰色，14pt）
  - [ ] SubTask 12.3: 用户按下 Ctrl+` 启动后提示消失

## 阶段五：交互补全

- [x] Task 13: 键盘快捷键补全
  - [ ] SubTask 13.1: `key_down_ctrl.rs` 添加 `VK_OEM_COMMA` → 打开设置侧边栏
  - [ ] SubTask 13.2: `key_down_ctrl.rs` 添加 `VK_J` → `layout.toggle_bottom_panel()`
  - [ ] SubTask 13.3: `key_down_ctrl.rs` 添加 `VK_T` + shift → 恢复最后关闭的标签（需新增 `last_closed_tab: Option<TabContent>` 状态）
  - [ ] SubTask 13.4: `key_down.rs` 添加 `VK_LEFT`/`VK_RIGHT` + Alt → 触发返回/前进导航
  - [ ] SubTask 13.5: `key_down_ctrl.rs` 添加 `VK_E` + shift → 切换到资源管理器视图
  - [ ] SubTask 13.6: `key_down_ctrl.rs` 添加 `VK_G` + shift → 切换到源代码管理视图（注意与现有 Ctrl+Shift+G 冲突，需调整）

- [x] Task 14: 活动栏右键菜单
  - [ ] SubTask 14.1: 在 `r_button_down.rs` 中活动栏命中时打开上下文菜单
  - [ ] SubTask 14.2: 菜单项：隐藏活动栏、自定义排序（进入 customize_mode）、分隔符、其他视图切换
  - [ ] SubTask 14.3: 复用 `context_menu.rs` 渲染方案

- [x] Task 15: 面板最小尺寸限制
  - [ ] SubTask 15.1: `layout.rs::resize_sidebar` 在 clamp 中使用 `MIN_SIDEBAR_WIDTH`（已存在 150.0），验证生效
  - [ ] SubTask 15.2: 新增 `MIN_BOTTOM_PANEL_HEIGHT: f32 = 100.0` 常量
  - [ ] SubTask 15.3: 新增 `MIN_RIGHT_PANEL_WIDTH: f32 = 150.0` 常量
  - [ ] SubTask 15.4: `resize_bottom_panel` 和 `resize_right_panel` 使用 clamp 强制最小值
  - [ ] SubTask 15.5: 拖拽到最小值后继续拖拽不缩小（鼠标位置与分隔条脱钩）

## 阶段六：集成测试与验证

- [x] Task 16: 编译与单元测试
  - [ ] SubTask 16.1: `cargo build --release` 编译通过
  - [ ] SubTask 16.2: `cargo test -p aether-win32` 所有现有测试通过
  - [ ] SubTask 16.3: 新增图标几何测试通过
  - [ ] SubTask 16.4: 新增 tooltip 状态测试通过

- [x] Task 17: 手动验收
  - [ ] SubTask 17.1: 启动应用，观察欢迎页 logo 为矢量羊形图标
  - [ ] SubTask 17.2: 打开文件夹，活动栏显示矢量图标
  - [ ] SubTask 17.3: 标题栏所有按钮显示矢量图标，hover 有颜色变化
  - [ ] SubTask 17.4: 悬停活动栏、工具按钮、状态栏分区 500ms 后显示 tooltip
  - [ ] SubTask 17.5: 鼠标移入编辑器时光标变为 IBEAM，移入按钮时变为 HAND
  - [ ] SubTask 17.6: 中键关闭标签生效
  - [ ] SubTask 17.7: 拖拽标签重排生效，显示插入指示线
  - [ ] SubTask 17.8: 右键标签显示上下文菜单，各菜单项功能正常
  - [ ] SubTask 17.9: Ctrl+, 打开设置，Ctrl+J 切换底部面板，Alt+Left/Right 导航
  - [ ] SubTask 17.10: 标签栏右侧 "+" 按钮新建标签
  - [ ] SubTask 17.11: 标签栏滚轮横向滚动生效
  - [ ] SubTask 17.12: 状态栏 Git 非 repo 时 Git 分支分区隐藏
  - [ ] SubTask 17.13: 欢迎页无最近项目时显示带图标引导
  - [ ] SubTask 17.14: 终端面板打开但未启动时显示提示文案
  - [ ] SubTask 17.15: 拖拽面板到最小尺寸后不再缩小

# Task Dependencies

- Task 2、3、4 依赖 Task 1（新图标几何）
- Task 5、6 互相独立，可并行
- Task 7、8、9 互相独立，可并行
- Task 10、11、12 互相独立，可并行
- Task 13、14、15 互相独立，可并行
- Task 16、17 依赖所有前序任务完成
