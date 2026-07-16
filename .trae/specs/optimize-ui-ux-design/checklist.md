# Checklist

## 视觉一致性

- [x] `IconKind` 枚举包含 `Back`、`Forward`、`Settings`、`User`、`Close`、`Plus`、`ChevronLeft`、`ChevronRight`、`EmojiSheep`、`Bot` 变体
- [x] 每个新 `IconKind` 在 `build_icon` 中有对应 PathGeometry 几何实现
- [x] `IconKind::ALL` 数组包含所有新变体
- [x] `ActivityBarView::icon()` 返回类型为 `IconKind`，不再返回 `&'static str`
- [x] `render_activity_bar` 使用 `IconCache::draw` 绘制图标，无 `DrawText` emoji 调用
- [x] `render_title_bar` 中返回/前进/关闭/设置/用户按钮使用矢量图标
- [x] 标题栏按钮无 `FillRectangle` 拼接像素点的代码
- [x] 欢迎页 logo 使用 `IconKind::EmojiSheep` 矢量图标，无 "🐑" emoji 字符

## 反馈与提示系统

- [x] 新增 `tooltip.rs` 模块定义 `TooltipState` 结构
- [x] `EditorState` 包含 `tooltip_state` 字段
- [x] 鼠标悬停 500ms 后显示 tooltip
- [x] 鼠标移动超过 4px 或离开元素时 tooltip 消失
- [x] tooltip 在最上层渲染（不被子元素遮挡）
- [x] tooltip 位置钳制在窗口范围内
- [x] 活动栏 hover 显示 tooltip（如 "资源管理器"、"源代码管理"）
- [x] 标题栏工具按钮 hover 显示 tooltip（如 "切换侧边栏"、"设置"、"用户菜单"）
- [x] 状态栏 clickable 分区 hover 显示 tooltip
- [x] 编辑器内容区鼠标光标为 `IDC_IBEAM`
- [x] 可点击元素鼠标光标为 `IDC_HAND`
- [x] 不可点击区域鼠标光标为 `IDC_ARROW`
- [x] `WM_SETCURSOR` 消息处理中调用 `SetCursor`

## 标签页交互

- [x] 中键点击标签关闭生效
- [x] 中键关闭活动标签时自动切换到相邻标签
- [x] 标签栏右侧显示 "+" 新建标签按钮
- [x] "+" 按钮点击后创建空白标签
- [x] 标签关闭按钮使用 `IconKind::Close` 矢量图标
- [x] 标签 dirty 状态显示为独立小圆点图标
- [x] 标签文件名文本不包含 "●" 字符
- [x] 标签栏鼠标滚轮横向滚动生效
- [x] 标签拖拽重排生效
- [x] 拖拽中显示插入位置指示线
- [x] 拖拽释放后标签顺序正确更新
- [x] 活动标签索引跟随拖拽移动
- [x] 右键标签显示上下文菜单
- [x] 菜单包含：关闭、关闭其他、关闭右侧、关闭所有、复制路径、在文件资源管理器中打开
- [x] 每个菜单项触发对应操作

## 视觉打磨

- [x] 活动栏 inactive 图标颜色为 0.55（提升对比度）
- [x] 状态栏 clickable 分区 hover 时背景高亮
- [x] Git 非 repo 时状态栏 Git 分支分区隐藏
- [x] 状态栏分区宽度根据文本测量自适应
- [x] 状态栏分区最小宽度 40px，最大 200px
- [x] 移除状态栏硬编码 width 魔法数字
- [x] 欢迎页无最近项目时显示带图标引导（文件夹图标 + 主文案 + 副文案 + 按钮）
- [x] 欢迎页空状态点击按钮触发打开文件夹对话框
- [x] 终端面板可见但未启动时显示 "按 Ctrl+` 启动终端" 提示
- [x] 终端启动后提示消失

## 键盘快捷键

- [x] `Ctrl+,` 打开设置侧边栏
- [x] `Ctrl+J` 切换底部面板可见性
- [x] `Ctrl+Shift+T` 恢复最后关闭的标签
- [x] `Alt+Left` 触发返回导航
- [x] `Alt+Right` 触发前进导航
- [x] `Ctrl+Shift+E` 切换到资源管理器视图
- [x] `Ctrl+Shift+G` 切换到源代码管理视图（与现有快捷键冲突已解决）

## 活动栏右键菜单

- [x] 右键活动栏显示上下文菜单
- [x] 菜单包含 "隐藏活动栏" 选项
- [x] 菜单包含 "自定义排序" 选项（进入 customize_mode）
- [x] 菜单包含其他视图切换选项
- [x] 复用 `context_menu.rs` 渲染方案

## 面板最小尺寸

- [x] 侧边栏宽度不低于 150px（拖拽时强制）
- [x] 底部面板高度不低于 100px（拖拽时强制）
- [x] 右侧面板宽度不低于 150px（拖拽时强制）
- [x] 拖拽到最小值后继续拖拽不缩小
- [x] `MIN_BOTTOM_PANEL_HEIGHT` 和 `MIN_RIGHT_PANEL_WIDTH` 常量已定义

## 编译与测试

- [x] `cargo build --release` 编译通过
- [x] `cargo test -p aether-win32` 所有现有测试通过
- [x] 新增图标几何测试通过
- [x] 新增 tooltip 状态测试通过
- [x] 无编译警告（`cargo clippy` 通过）

## 回归验证

- [x] 原有功能不受影响：打开文件、编辑、保存、查找替换、命令面板、Git 集成、SSH、AI 面板
- [x] 原有快捷键不受影响：Ctrl+S/O/K/N/B/P/`/=/-/0/G 等
- [x] 标签切换、关闭、新建（Ctrl+T）原有行为保持
- [x] 活动栏视图切换、自定义排序原有行为保持
- [x] 菜单栏展开、子菜单、自定义排序原有行为保持
- [x] 状态栏点击各分区原有行为保持
