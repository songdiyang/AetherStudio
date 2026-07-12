# 文件类型图标规范 · Aether Studio

> `.py` `.java` `.txt` 矢量图标系统 — 设计资产 + 集成规范

## 1. 资产总览

| 文件类型 | 风格 | 文件路径 |
|---------|------|---------|
| Python  | Flat  | `crates/aether-win32/resources/file_icons/flat/python.svg` |
| Python  | Minimal  | `crates/aether-win32/resources/file_icons/minimal/python.svg` |
| Python  | Skeuomorphic  | `crates/aether-win32/resources/file_icons/skeuomorphic/python.svg` |
| Java    | Flat  | `crates/aether-win32/resources/file_icons/flat/java.svg` |
| Java    | Minimal  | `crates/aether-win32/resources/file_icons/minimal/java.svg` |
| Java    | Skeuomorphic  | `crates/aether-win32/resources/file_icons/skeuomorphic/java.svg` |
| Text    | Flat  | `crates/aether-win32/resources/file_icons/flat/text.svg` |
| Text    | Minimal  | `crates/aether-win32/resources/file_icons/minimal/text.svg` |
| Text    | Skeuomorphic  | `crates/aether-win32/resources/file_icons/skeuomorphic/text.svg` |
| 预览页  | — | `crates/aether-win32/resources/file_icons/preview/index.html` |

**全部共 9 个 SVG + 1 个浏览器预览页。**

## 2. 设计参数

| 项目 | 值 |
|------|-----|
| 视口 | 24 × 24 |
| 缩放 | viewBox 自适应，从 16px → 32px+ 均无锯齿 |
| 笔画（Minimal） | 1.5（视口单位），圆头圆角 |
| 文件外形 | `(5.5, 2.5) → (20.5, 21.5)`，圆角 1.0 |
| 折角位置 | 右上角 `(14.5, 2.5) → (20.5, 8.5)` |

## 3. 三种风格对比

### 3.1 Flat（扁平化）
- **定位**：现代应用首选，色彩鲜明
- **特征**：纯色填充 + 几何形态 + 简化符号
- **色板**：
  - Python：蓝 `#3776AB` + 黄 `#FFD43B`
  - Java：橙 `#E76F00` + 白
  - Text：灰 `#6B7280` + 白
- **适用**：明亮主题、品牌展示页、活动栏

### 3.2 Minimal（极简）— **当前已集成**
- **定位**：与现有图标系统一致，单色描边
- **特征**：1.5px 描边 + currentColor + 无填充
- **色板**：跟随主题色（CSS `currentColor` / D2D 画刷）
- **适用**：侧边栏、文件树、状态栏 — 全部 UI
- **集成位置**：`crates/aether-win32/src/icons.rs::IconKind::{FilePython, FileJava, FileText}`

### 3.3 Skeuomorphic（拟物化）
- **定位**：高辨识度视觉冲击
- **特征**：线性/径向渐变 + 投影 + 顶部高光
- **色板**：品牌色深-浅渐变对
- **适用**：欢迎页、Splash、关于对话框

## 4. 设计语义说明

| 文件 | 视觉元素 | 设计意图 |
|------|---------|---------|
| **Python** | 双圆互锁 + 对角连线 | 复刻 Python 官方 logo 简化版，体现"双蛇互锁"语言特性 |
| **Java** | 咖啡杯 + 把手 + 蒸汽 | Java 经典咖啡杯图腾，"Write once, run anywhere"的温暖感 |
| **Text** | 三条横线（短-长-短） | 直观表达"纯文本段落"，通用文档抽象 |

## 5. 多尺寸可识别性

| 尺寸 | 设计考量 | 验证方法 |
|------|---------|---------|
| **16 × 16** | 移除装饰细节，保留核心符号；双圆半径 ≥ 2.5 | 浏览器缩放至 16px 验证 |
| **24 × 24** | 设计基准尺寸，所有细节完整 | 默认渲染 |
| **32 × 32** | 笔画 + 细节 + 装饰（高光/阴影）全部呈现 | 浏览器缩放至 32px 验证 |

> 关键策略：核心符号在 16px 下仍能 ≥ 5px，确保 ≥ 2px 笔画不消失。

## 6. 代码集成

### 6.1 现有架构
- 图标系统：`crates/aether-win32/src/icons.rs`
- 渲染后端：Direct2D `ID2D1PathGeometry`
- 视口：24×24，绘制时 `icons.draw(...)` 缩放到目标矩形

### 6.2 新增变体
```rust
pub enum IconKind {
    // ... 原有 38 种
    FilePython,  // .py / .pyw / .pyi
    FileJava,    // .java / .kt
    FileText,    // .txt
}

pub const ALL: [IconKind; 41] = [ /* ... 原有 + 3 */ ];
```

### 6.3 文件树使用流程
1. `render_file_tree_sidebar` → `icons.ensure_created_from_target(target)` 初始化几何
2. `get_file_vector_icon(name)` → `Option<IconKind>`
3. 命中 `.py/.java/.txt` 时：
   - 文本前缀 emoji 位置留空
   - 在 `item_left - 2.0` 处绘制 16×16 矢量图标
4. 未命中 → 回退到原 emoji 字符串

### 6.4 扩展新风格支持（未来）

若需要启用 Flat / Skeuomorphic，需要扩展 `IconCache` 支持多色填充：
```rust
// 伪代码：未来扩展方向
pub struct FileIconCache {
    flat_geometries: HashMap<IconKind, ColoredGeometry>,
    minimal_geometries: HashMap<IconKind, ID2D1PathGeometry>,
}
pub enum FileIconStyle { Flat, Minimal, Skeuomorphic }
```

## 7. 浏览器预览

打开 `crates/aether-win32/resources/file_icons/preview/index.html` 可在浏览器中查看：
- 三风格 × 三尺寸的并排对比
- 模拟深色主题下文件树中的实际显示效果

## 8. 维护规则

1. **新增文件类型图标**：在 `IconKind` 中添加变体 + 更新 `get_file_vector_icon` 映射
2. **设计变更**：先更新 SVG（设计源），再同步更新 `icons.rs` 的 PathGeometry
3. **尺寸调整**：在 `preview/index.html` 中验证 16/24/32px 三档可读性
4. **主题适配**：Minimal 风格通过 currentColor 自动适配；Flat/Skeuo 需要主题切换时同步更新色板

## 9. 已知约束

- 当前 `IconCache` 单色描边设计使 Flat / Skeuomorphic 暂未集成到运行时，仅作设计资产
- Direct2D `ID2D1PathGeometry` 不支持渐变填充；启用拟物化需扩展 `LinearGradientBrush` 支持
- emoji 字符串 `get_file_icon` 仍保留作为其他文件类型的回退方案
