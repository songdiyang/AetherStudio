//! 全部 UI / 文件类型图标 SVG 定义
//!
//! 来源（全部 ISC / MIT 许可证，商业友好）：
//! - Lucide Icons (ISC)  https://lucide.dev
//! - Devicon (MIT)       https://devicon.dev
//!
//! 所有图标统一为 24x24 viewBox，渲染时按目标尺寸缩放。
//!
//! 数据格式：
//! - 单色 stroke 风格（Lucide 风格）：fill = None，使用当前 brush 描边
//! - 多色 fill 风格（Devicon 风格）：每个 shape 携带 hex 颜色

/// SVG 形状（用于嵌入到 PathGeometry 几何）
#[derive(Clone, Copy)]
pub(crate) enum SvgShape {
    /// SVG path d="..." 字符串；fill = None 表示 stroke 模式，Some(hex) 表示 fill 模式
    Path(&'static str, Option<&'static str>),
    /// 圆形：cx, cy, r, fill
    Circle(f32, f32, f32, Option<&'static str>),
    /// 矩形：x, y, w, h, fill, rx（圆角，可选）
    Rect(f32, f32, f32, f32, Option<&'static str>, Option<f32>),
    /// 直线：x1, y1, x2, y2
    Line(f32, f32, f32, f32),
}

/// 一个完整的 SVG 图标定义
#[derive(Clone, Copy)]
pub(crate) struct SvgDef {
    /// 视图框 (x, y, w, h)，通常 (0, 0, 24, 24)
    pub viewbox: (f32, f32, f32, f32),
    /// 该图标包含的形状
    pub shapes: &'static [SvgShape],
}

// ===========================================================================
// UI 图标（Lucide 风格，stroke 模式 — fill = None）
// ===========================================================================

/// Lucide "folder-open" - 打开的文件夹
const UI_FOLDER_OPEN: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2", None),
    ],
};

/// Lucide "folder" - 关闭的文件夹
const UI_FOLDER: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z", None),
    ],
};

/// Lucide "file-plus" - 新建文件
const UI_NEW_FILE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", None),
        SvgShape::Path("M14 2v5a1 1 0 0 0 1 1h5", None),
        SvgShape::Path("M9 15h6", None),
        SvgShape::Path("M12 18v-6", None),
    ],
};

/// Lucide "file" - 普通文件
const UI_FILE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", None),
        SvgShape::Path("M14 2v5a1 1 0 0 0 1 1h5", None),
    ],
};

/// Lucide "save" - 保存（软盘）
const UI_SAVE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", None),
        SvgShape::Path("M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7", None),
        SvgShape::Path("M7 3v4a1 1 0 0 0 1 1h7", None),
    ],
};

/// Lucide "copy" - 复制（两个重叠矩形）
const UI_COPY: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Rect(8.0, 8.0, 14.0, 14.0, None, Some(2.0)),
        SvgShape::Path(
            "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
            None,
        ),
    ],
};

/// Lucide "scissors" - 剪切
const UI_CUT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Circle(6.0, 6.0, 3.0, None),
        SvgShape::Path("M8.12 8.12 12 12", None),
        SvgShape::Path("M20 4 8.12 15.88", None),
        SvgShape::Circle(6.0, 18.0, 3.0, None),
        SvgShape::Path("M14.8 14.8 20 20", None),
    ],
};

/// Lucide "clipboard-paste" - 粘贴
const UI_PASTE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M11 14h10", None),
        SvgShape::Path("M16 4h2a2 2 0 0 1 2 2v1.344", None),
        SvgShape::Path("m17 18 4-4-4-4", None),
        SvgShape::Path(
            "M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 1.793-1.113",
            None,
        ),
        SvgShape::Rect(8.0, 2.0, 8.0, 4.0, None, Some(1.0)),
    ],
};

/// Lucide "list-checks" - 全选
const UI_SELECT_ALL: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M13 5h8", None),
        SvgShape::Path("M13 12h8", None),
        SvgShape::Path("M13 19h8", None),
        SvgShape::Path("m3 17 2 2 4-4", None),
        SvgShape::Path("m3 7 2 2 4-4", None),
    ],
};

/// Lucide "search" - 查找
const UI_SEARCH: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("m21 21-4.34-4.34", None),
        SvgShape::Circle(11.0, 11.0, 8.0, None),
    ],
};

/// Lucide "replace" - 替换
const UI_REPLACE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M14 4a1 1 0 0 1 1-1", None),
        SvgShape::Path("M15 10a1 1 0 0 1-1-1", None),
        SvgShape::Path("M21 4a1 1 0 0 0-1-1", None),
        SvgShape::Path("M21 9a1 1 0 0 1-1 1", None),
        SvgShape::Path("m3 7 3 3 3-3", None),
        SvgShape::Path("M6 10V5a2 2 0 0 1 2-2h2", None),
        SvgShape::Rect(3.0, 14.0, 7.0, 7.0, None, Some(1.0)),
    ],
};

/// Lucide "undo-2" - 撤销
const UI_UNDO: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M9 14 4 9l5-5", None),
        SvgShape::Path(
            "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11",
            None,
        ),
    ],
};

/// Lucide "redo-2" - 重做
const UI_REDO: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("m15 14 5-5-5-5", None),
        SvgShape::Path(
            "M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13",
            None,
        ),
    ],
};

/// Lucide "panel-left" - 侧边栏
const UI_SIDEBAR: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Rect(3.0, 3.0, 18.0, 18.0, None, Some(2.0)),
        SvgShape::Path("M9 3v18", None),
    ],
};

/// Lucide "panel-left-open" - 左侧面板
const UI_PANEL_LEFT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Rect(3.0, 3.0, 18.0, 18.0, None, Some(2.0)),
        SvgShape::Path("M9 3v18", None),
        SvgShape::Path("m14 9 3 3-3 3", None),
    ],
};

/// Lucide "panel-bottom" - 底部面板
const UI_PANEL_BOTTOM: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Rect(3.0, 3.0, 18.0, 18.0, None, Some(2.0)),
        SvgShape::Path("M3 15h18", None),
    ],
};

/// Lucide "hash" - # 符号
const UI_HASH: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Line(4.0, 9.0, 20.0, 9.0),
        SvgShape::Line(4.0, 15.0, 20.0, 15.0),
        SvgShape::Line(10.0, 3.0, 8.0, 21.0),
        SvgShape::Line(16.0, 3.0, 14.0, 21.0),
    ],
};

/// Lucide "play" - 播放
const UI_PLAY: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[SvgShape::Path(
        "M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z",
        None,
    )],
};

/// Lucide "bug" - 调试
const UI_BUG: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 20v-9", None),
        SvgShape::Path(
            "M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z",
            None,
        ),
        SvgShape::Path("M14.12 3.88 16 2", None),
        SvgShape::Path("M21 21a4 4 0 0 0-3.81-4", None),
        SvgShape::Path("M21 5a4 4 0 0 1-3.55 3.97", None),
        SvgShape::Path("M22 13h-4", None),
        SvgShape::Path("M3 21a4 4 0 0 1 3.81-4", None),
        SvgShape::Path("M3 5a4 4 0 0 0 3.55 3.97", None),
        SvgShape::Path("M6 13H2", None),
        SvgShape::Path("m8 2 1.88 1.88", None),
        SvgShape::Path("M9 7.13V6a3 3 0 1 1 6 0v1.13", None),
    ],
};

/// Lucide "terminal" - 终端
const UI_TERMINAL: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 19h8", None),
        SvgShape::Path("m4 17 6-6-6-6", None),
    ],
};

/// Lucide "git-branch" - Git 分支
const UI_GIT_BRANCH: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M15 6a9 9 0 0 0-9 9V3", None),
        SvgShape::Circle(18.0, 6.0, 3.0, None),
        SvgShape::Circle(6.0, 18.0, 3.0, None),
    ],
};

/// Lucide "circle-x" - 错误
const UI_ERROR: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Circle(12.0, 12.0, 10.0, None),
        SvgShape::Path("m15 9-6 6", None),
        SvgShape::Path("m9 9 6 6", None),
    ],
};

/// Lucide "triangle-alert" - 警告
const UI_WARNING: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path(
            "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
            None,
        ),
        SvgShape::Path("M12 9v4", None),
        SvgShape::Path("M12 17h.01", None),
    ],
};

/// Lucide "info" - 信息
const UI_INFO: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Circle(12.0, 12.0, 10.0, None),
        SvgShape::Path("M12 16v-4", None),
        SvgShape::Path("M12 8h.01", None),
    ],
};

/// Lucide "log-out" - 退出
const UI_EXIT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("m16 17 5-5-5-5", None),
        SvgShape::Path("M21 12H9", None),
        SvgShape::Path("M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4", None),
    ],
};

/// Lucide "arrow-left" - 返回
const UI_BACK: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("m12 19-7-7 7-7", None),
        SvgShape::Path("M19 12H5", None),
    ],
};

/// Lucide "arrow-right" - 前进
const UI_FORWARD: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5 12h14", None),
        SvgShape::Path("m12 5 7 7-7 7", None),
    ],
};

/// Lucide "settings" - 设置（齿轮）
const UI_SETTINGS: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915", None),
        SvgShape::Circle(12.0, 12.0, 3.0, None),
    ],
};

/// Lucide "user" - 用户
const UI_USER: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2", None),
        SvgShape::Circle(12.0, 7.0, 4.0, None),
    ],
};

/// Lucide "x" - 关闭
const UI_CLOSE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M18 6 6 18", None),
        SvgShape::Path("m6 6 12 12", None),
    ],
};

/// Lucide "plus" - 加号
const UI_PLUS: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5 12h14", None),
        SvgShape::Path("M12 5v14", None),
    ],
};

/// Lucide "chevron-left" - 左折角
const UI_CHEVRON_LEFT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[SvgShape::Path("m15 18-6-6 6-6", None)],
};

/// Lucide "chevron-right" - 右折角
const UI_CHEVRON_RIGHT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[SvgShape::Path("m9 18 6-6-6-6", None)],
};

/// Lucide "bot" - 机器人（AI 助手）
const UI_BOT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 8V4H8", None),
        SvgShape::Rect(4.0, 8.0, 16.0, 12.0, None, Some(2.0)),
        SvgShape::Path("M2 14h2", None),
        SvgShape::Path("M20 14h2", None),
        SvgShape::Path("M15 13v2", None),
        SvgShape::Path("M9 13v2", None),
    ],
};

/// Lucide "plug" - SSH/插头
const UI_SSH: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 22v-5", None),
        SvgShape::Path("M15 8V2", None),
        SvgShape::Path(
            "M17 8a1 1 0 0 1 1 1v4a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1z",
            None,
        ),
        SvgShape::Path("M9 8V2", None),
    ],
};

/// Lucide "git-fork" - 克隆（fork 风格）
const UI_CLONE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Circle(6.0, 3.0, 2.0, None),
        SvgShape::Circle(6.0, 21.0, 2.0, None),
        SvgShape::Circle(18.0, 12.0, 2.0, None),
        SvgShape::Path("M6 5v14", None),
        SvgShape::Path("M6 11a6 6 0 0 0 6 6h0a4 4 0 0 1 4-4", None),
    ],
};

/// Lucide "file-search" - 转到文件
const UI_GOTO_FILE: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", None),
        SvgShape::Path("M14 2v5a1 1 0 0 0 1 1h5", None),
        SvgShape::Circle(11.5, 14.5, 2.5, None),
        SvgShape::Path("M13.3 16.3 15 18", None),
    ],
};

/// 自定义 - 羊脸（Lucide 无等价物，使用 SVG 路径近似）
const UI_EMOJI_SHEEP: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        // 头部圆
        SvgShape::Circle(12.0, 13.0, 7.0, None),
        // 左耳
        SvgShape::Circle(4.0, 11.0, 2.0, None),
        // 右耳
        SvgShape::Circle(20.0, 11.0, 2.0, None),
        // 左眼
        SvgShape::Circle(9.0, 11.0, 0.9, Some("#1F2328")),
        // 右眼
        SvgShape::Circle(15.0, 11.0, 0.9, Some("#1F2328")),
        // 顶部绒毛弧
        SvgShape::Path("M10 6 Q12 3 14 6", None),
    ],
};

// ===========================================================================
// 文件类型图标（彩色 fill 模式 — Devicon / Material Theme 风格）
// ===========================================================================

/// 通用文件型 SVG：右上折角文件 + 内嵌符号
/// 在 fill 模式下，path 元素携带 fill 颜色
const FILE_TEXT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        // 文件体
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#5A5A5A")),
        // 折角
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 三条文本行
        SvgShape::Path("M7.5 12 L16.5 12", Some("#FFFFFF")),
        SvgShape::Path("M7.5 15 L16.5 15", Some("#FFFFFF")),
        SvgShape::Path("M7.5 18 L13 18", Some("#FFFFFF")),
    ],
};

/// Python 文件图标（基于 Material Theme Python）
const FILE_PYTHON: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        // 文件体 - Python 蓝
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#3776AB")),
        // 折角 - Python 黄
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFD43B")),
        // 双圆互锁
        SvgShape::Circle(9.5, 15.0, 3.0, Some("#FFD43B")),
        SvgShape::Circle(14.5, 15.0, 3.0, Some("#FFFFFF")),
    ],
};

/// Java 文件图标（基于 Material Theme Java）
const FILE_JAVA: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        // 文件体 - Java 红
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#E76F00")),
        // 折角
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 咖啡杯主体（梯形）
        SvgShape::Path("M8 12 L16 12 L15.2 18.5 Q15.1 19.5 14.1 19.5 L9.9 19.5 Q8.9 19.5 8.8 18.5 Z", Some("#FFFFFF")),
        // 杯把手
        SvgShape::Path("M16 14 Q18.5 14 18.5 16 Q18.5 18 16 18", Some("#FFFFFF")),
        // 蒸汽
        SvgShape::Path("M10 9 Q11 7.5 10 6", Some("#FFFFFF")),
        SvgShape::Path("M14 9 Q15 7.5 14 6", Some("#FFFFFF")),
    ],
};

/// C 文件图标（基于 Devicon C 风格）
const FILE_C: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#5879A2")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // C 字符（马蹄形）
        SvgShape::Path("M15.5 12.5 Q14 11 9 11 Q9 15.5 9 20 Q14 20 15.5 18.5 L14.5 17.5 Q13 19 11 19 Q10.5 19 10.5 15.5 Q10.5 12 11 12 Q13 12 14.5 13.5 Z", Some("#FFFFFF")),
    ],
};

/// C++ 文件图标
const FILE_CPP: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#00599C")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // C
        SvgShape::Path("M12 12.5 Q11 11.5 8 11.5 Q8 15.5 8 19.5 Q11 19.5 12 18.5 L11.2 17.7 Q10 19 9.2 19 Q8.9 19 8.9 15.5 Q8.9 12 9.2 12 Q10 12 11.2 13.3 Z", Some("#FFFFFF")),
        // 两个 +
        SvgShape::Path("M14 13.5 L16 13.5 M15 12.5 L15 14.5", Some("#FFFFFF")),
        SvgShape::Path("M17 17 L19 17 M18 16 L18 18", Some("#FFFFFF")),
    ],
};

/// C# 文件图标
const FILE_CSHARP: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#68217A")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // # 符号
        SvgShape::Path("M10 11 L11 11 L9.5 20 L8.5 20 Z", Some("#FFFFFF")),
        SvgShape::Path("M14 11 L15 11 L13.5 20 L12.5 20 Z", Some("#FFFFFF")),
        SvgShape::Path("M8 13.5 L15 13 L15 14 L8 14.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M7.5 17 L14.5 16.5 L14.5 17.5 L7.5 18 Z", Some("#FFFFFF")),
    ],
};

/// Go 文件图标
const FILE_GO: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#00ADD8")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // G 字
        SvgShape::Path("M15 12.5 Q14 11 9.5 11 Q9.5 15.5 9.5 20 Q14 20 15 18.5 L15 15.5 L12 15.5 L12 16.5 L14 16.5 L14 18 Q13 19 11 19 Q10.5 19 10.5 15.5 Q10.5 12 11 12 Q13 12 14 13.5 Z", Some("#FFFFFF")),
    ],
};

/// Rust 文件图标（基于 Devicon Rust 风格）
const FILE_RUST: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#DEA584")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 简化的齿轮（rust 标识是齿轮）
        SvgShape::Circle(12.5, 15.5, 3.8, Some("#FFFFFF")),
        SvgShape::Circle(12.5, 15.5, 1.5, Some("#DEA584")),
        SvgShape::Path("M11 9.5 L11 11 L14 11 L14 9.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M17.5 12 L15.5 13 L16.5 15.5 L18.5 14.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M17.5 19 L15.5 18 L14.5 20.5 L16.5 21.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M11 21.5 L11 20 L14 20 L14 21.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M7.5 19 L9.5 18 L10.5 20.5 L8.5 21.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M7.5 12 L9.5 13 L8.5 15.5 L6.5 14.5 Z", Some("#FFFFFF")),
    ],
};

/// JavaScript 文件图标
const FILE_JS: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#F7DF1E")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#000000")),
        // JS 字符
        SvgShape::Path("M9 11 L10.5 11 L10.5 17.5 Q10.5 18.5 9.5 18.5 Q8.5 18.5 8 17.5 L7 18 Q7.8 19.5 9.5 19.5 Q11.5 19.5 11.5 17.5 L11.5 11 Z", Some("#000000")),
        SvgShape::Path("M13 18 L14 17.5 Q14.5 18.5 15.5 18.5 Q16.5 18.5 16.5 17.5 Q16.5 16.5 15 16 Q13 15.5 13 14 Q13 12.5 14.5 12.5 Q15.8 12.5 16.5 13.5 L15.5 14 Q15 13.5 14.5 13.5 Q14 13.5 14 14 Q14 14.7 15.5 15 Q17 15.5 17 17 Q17 19 15.5 19 Q14 19 13 18 Z", Some("#000000")),
    ],
};

/// TypeScript 文件图标
const FILE_TS: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#3178C6")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // TS 字符
        SvgShape::Path("M7.5 11 L13.5 11 L13.5 12.5 L11.25 12.5 L11.25 19 L9.75 19 L9.75 12.5 L7.5 12.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M14.5 17.3 L15.7 16.7 Q16 17.5 16.8 17.5 Q17.6 17.5 17.6 16.7 Q17.6 15.9 16 15.5 Q14.3 15.1 14.3 13.7 Q14.3 12.3 15.7 12.3 Q16.9 12.3 17.6 13.3 L16.5 14 Q16.2 13.5 15.7 13.5 Q15.2 13.5 15.2 14 Q15.2 14.6 16.8 15 Q18.5 15.4 18.5 16.9 Q18.5 18.5 16.9 18.5 Q15.4 18.5 14.5 17.3 Z", Some("#FFFFFF")),
    ],
};

/// HTML 文件图标
const FILE_HTML: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#E34F26")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // <> 字符
        SvgShape::Path("M8 11.5 L10 15.5 L8 19.5 L6.5 19.5 L8.5 15.5 L6.5 11.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M16 11.5 L17.5 11.5 L15.5 15.5 L17.5 19.5 L16 19.5 L14 15.5 Z", Some("#FFFFFF")),
    ],
};

/// CSS 文件图标
const FILE_CSS: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#1572B6")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // {} 字符
        SvgShape::Path("M8.5 11 L10 11 L9 14 L10 14 L10 15 L9 15 L10 19 L8.5 19 L7.5 15 L8.5 14.5 L8.5 14 L7.5 14 Z", Some("#FFFFFF")),
        SvgShape::Path("M15.5 11 L16.5 19 L15 19 L14.5 15 L15.5 14.5 L15.5 14 L14.5 11 Z", Some("#FFFFFF")),
    ],
};

/// JSON 文件图标
const FILE_JSON: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#5A5A5A")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // {} 字符
        SvgShape::Path("M8 11 L9.5 11 L8.5 14.5 L9.5 14.5 L9.5 15.5 L8.5 15.5 L9.5 19 L8 19 L7 15 L8 15 L7 11 Z", Some("#FFFFFF")),
        SvgShape::Path("M15.5 11 L16.5 19 L15 19 L14.5 15.5 L15.5 15.5 L14.5 11 Z", Some("#FFFFFF")),
    ],
};

/// YAML 文件图标
const FILE_YAML: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#CB171E")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // Y 字符
        SvgShape::Path("M8 11 L9.5 11 L11.5 14 L13.5 11 L15 11 L12 15.5 L12 19.5 L11 19.5 L11 15.5 Z", Some("#FFFFFF")),
    ],
};

/// TOML 文件图标
const FILE_TOML: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#9C4221")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // T 字符
        SvgShape::Path("M7 11 L14.5 11 L14.5 12.5 L11.5 12.5 L11.5 19.5 L10 19.5 L10 12.5 L7 12.5 Z", Some("#FFFFFF")),
    ],
};

/// Markdown 文件图标
const FILE_MARKDOWN: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#083FA1")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // MD 字符
        SvgShape::Rect(6.5, 11.0, 2.0, 8.0, Some("#FFFFFF"), None),
        SvgShape::Rect(15.5, 11.0, 2.0, 8.0, Some("#FFFFFF"), None),
        SvgShape::Path("M9.5 14 L11 15.5 L12.5 14 L12.5 19 L11.5 19 L11.5 16 L11 16.5 L10.5 16 L10.5 19 L9.5 19 Z", Some("#FFFFFF")),
        SvgShape::Path("M14 13 L15.5 14.5 L15.5 19 L14.5 19 L14.5 15.5 L13.5 16 L13 15.5 Z", Some("#FFFFFF")),
    ],
};

/// Shell 脚本文件图标
const FILE_SHELL: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#4EAA25")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // $ 字符
        SvgShape::Path("M14.5 11 L14.5 12.3 L12.5 12.3 L12.5 14.3 L14.5 14.3 L14.5 17.5 L12 17.5 L12 16.2 L13.5 16.2 L13.5 15.2 L11.5 15.2 L11.5 11.5 L14.5 11.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M12.8 9.5 L13.5 9.5 L13.5 19.5 L12.8 19.5 Z", Some("#FFFFFF")),
        // > 字符
        SvgShape::Path("M6.5 13 L8 14.5 L6.5 16 L6 15.5 L7 14.5 L6 13.5 Z", Some("#FFFFFF")),
        SvgShape::Path("M8.5 16.5 L11 16.5 L11 17.5 L8.5 17.5 Z", Some("#FFFFFF")),
    ],
};

/// SQL 文件图标（数据库造型）
const FILE_SQL: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#E38C00")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 数据库（圆柱）
        SvgShape::Path("M8 13 Q8 11.5 12.5 11.5 Q17 11.5 17 13 L17 17 Q17 18.5 12.5 18.5 Q8 18.5 8 17 Z", Some("#FFFFFF")),
        SvgShape::Path("M8 13 Q8 14.5 12.5 14.5 Q17 14.5 17 13", Some("#E38C00")),
        SvgShape::Path("M8 15 Q8 16.5 12.5 16.5 Q17 16.5 17 15", Some("#E38C00")),
    ],
};

/// Ruby 文件图标（红宝石造型）
const FILE_RUBY: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#CC342D")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 钻石/红宝石造型
        SvgShape::Path("M12 11 L16.5 11 L18 13.5 L12 19.5 L6 13.5 L7.5 11 Z", Some("#FFFFFF")),
        SvgShape::Path("M12 11 L12 19.5", Some("#CC342D")),
        SvgShape::Path("M7.5 11 L16.5 11", Some("#CC342D")),
    ],
};

/// PHP 文件图标
const FILE_PHP: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#777BB4")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // P 字符
        SvgShape::Path("M8 11 L11.5 11 Q14 11 14 13.5 Q14 16 11.5 16 L9.5 16 L9.5 19.5 L8 19.5 Z M9.5 12.5 L9.5 14.5 L11 14.5 Q12 14.5 12 13.5 Q12 12.5 11 12.5 Z", Some("#FFFFFF")),
    ],
};

/// Lua 文件图标
const FILE_LUA: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#000080")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // L 字符
        SvgShape::Path("M9 11 L10.5 11 L10.5 18 L15 18 L15 19.5 L9 19.5 Z", Some("#FFFFFF")),
    ],
};

/// Swift 文件图标
const FILE_SWIFT: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#FA7343")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // Swift 鸟翅膀简化
        SvgShape::Path("M8 19 L18 11 L13 11 L9 14 L8 19 Z", Some("#FFFFFF")),
        SvgShape::Path("M8 19 L15 13 L11 13 L8 16 L8 19 Z", Some("#FA7343")),
    ],
};

/// Kotlin 文件图标
const FILE_KOTLIN: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#7F52FF")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // K 字符
        SvgShape::Path("M8.5 11 L10 11 L10 14.5 L13 11 L15 11 L11.5 15 L15 19.5 L13 19.5 L10 15.5 L10 19.5 L8.5 19.5 Z", Some("#FFFFFF")),
    ],
};

/// Docker 文件图标（鲸鱼/集装箱简化）
const FILE_DOCKER: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M5.5 2.5 L14.5 2.5 L20.5 8.5 L20.5 20.5 Q20.5 21.5 19.5 21.5 L5.5 21.5 Q4.5 21.5 4.5 20.5 L4.5 3.5 Q4.5 2.5 5.5 2.5 Z", Some("#384D54")),
        SvgShape::Path("M14.5 2.5 L20.5 8.5 L15.5 8.5 Q14.5 8.5 14.5 7.5 Z", Some("#FFFFFF")),
        // 集装箱
        SvgShape::Rect(7.0, 13.0, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Rect(9.7, 13.0, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Rect(12.4, 13.0, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Rect(7.0, 15.7, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Rect(9.7, 15.7, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Rect(7.0, 10.3, 2.5, 2.5, Some("#FFFFFF"), None),
        SvgShape::Path("M15 18.5 Q18 18.5 18 16.5 Q17 16.5 16.5 17 Q16 17 15.5 17.5", Some("#FFFFFF")),
    ],
};

// ===========================================================================
// AI 面板输入框工具栏图标（Lucide 风格，stroke 模式）
// ===========================================================================

/// Lucide "send" - 发送（纸飞机/箭头）
const UI_SEND: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M22 2L11 13", None),
        SvgShape::Path("M22 2l-7 20-4-9-9-4 20-7z", None),
    ],
};

/// Lucide "mic" - 麦克风
const UI_MIC: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 19v3", None),
        SvgShape::Path("M8 12v1a4 4 0 0 0 8 0v-1", None),
        SvgShape::Path("M12 19c-2.8 0-5-2.2-5-5v-4", None),
        SvgShape::Path("M17 8v4a5 5 0 0 1-10 0V8", None),
        SvgShape::Path("M12 1a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V4a3 3 0 0 1 3-3z", None),
    ],
};

/// Lucide "sparkles" - 星星/闪光
const UI_SPARKLES: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M12 2l1.5 4.5L18 8l-4.5 1.5L12 14l-1.5-4.5L6 8l4.5-1.5z", None),
        SvgShape::Path("M18 12l.8 2.4L21 15l-2.4.8L18 18l-.8-2.4L15 15l2.4-.8z", None),
    ],
};

/// Lucide "list" - 菜单/列表
const UI_LIST: SvgDef = SvgDef {
    viewbox: (0.0, 0.0, 24.0, 24.0),
    shapes: &[
        SvgShape::Path("M8 6h13", None),
        SvgShape::Path("M8 12h13", None),
        SvgShape::Path("M8 18h13", None),
        SvgShape::Path("M3 6h.01", None),
        SvgShape::Path("M3 12h.01", None),
        SvgShape::Path("M3 18h.01", None),
    ],
};

// ===========================================================================
// 图标定义表（按 IconKind 索引）
// ===========================================================================

/// 全图标定义表，索引与 IconKind::ALL 一致
pub(crate) const SVG_DEFS: &[SvgDef] = &[
    /*  0 OpenFolder    */ UI_FOLDER_OPEN,
    /*  1 NewFile       */ UI_NEW_FILE,
    /*  2 Clone         */ UI_CLONE,
    /*  3 Ssh           */ UI_SSH,
    /*  4 Folder        */ UI_FOLDER,
    /*  5 File          */ UI_FILE,
    /*  6 Save          */ UI_SAVE,
    /*  7 Undo          */ UI_UNDO,
    /*  8 Redo          */ UI_REDO,
    /*  9 Cut           */ UI_CUT,
    /* 10 Copy          */ UI_COPY,
    /* 11 Paste         */ UI_PASTE,
    /* 12 SelectAll     */ UI_SELECT_ALL,
    /* 13 Search        */ UI_SEARCH,
    /* 14 Replace       */ UI_REPLACE,
    /* 15 Sidebar       */ UI_SIDEBAR,
    /* 16 PanelLeft     */ UI_PANEL_LEFT,
    /* 17 PanelBottom   */ UI_PANEL_BOTTOM,
    /* 18 GotoFile      */ UI_GOTO_FILE,
    /* 19 Hash          */ UI_HASH,
    /* 20 Play          */ UI_PLAY,
    /* 21 Bug           */ UI_BUG,
    /* 22 Terminal      */ UI_TERMINAL,
    /* 23 GitBranch     */ UI_GIT_BRANCH,
    /* 24 Error         */ UI_ERROR,
    /* 25 Warning       */ UI_WARNING,
    /* 26 Info          */ UI_INFO,
    /* 27 Exit          */ UI_EXIT,
    /* 28 Back          */ UI_BACK,
    /* 29 Forward       */ UI_FORWARD,
    /* 30 Settings      */ UI_SETTINGS,
    /* 31 User          */ UI_USER,
    /* 32 Close         */ UI_CLOSE,
    /* 33 Plus          */ UI_PLUS,
    /* 34 ChevronLeft   */ UI_CHEVRON_LEFT,
    /* 35 ChevronRight  */ UI_CHEVRON_RIGHT,
    /* 36 EmojiSheep    */ UI_EMOJI_SHEEP,
    /* 37 Bot           */ UI_BOT,
    /* 38 Send          */ UI_SEND,
    /* 39 Mic           */ UI_MIC,
    /* 40 Sparkles      */ UI_SPARKLES,
    /* 41 List          */ UI_LIST,
    /* 42 FilePython    */ FILE_PYTHON,
    /* 43 FileJava      */ FILE_JAVA,
    /* 44 FileText      */ FILE_TEXT,
    /* 45 FileC         */ FILE_C,
    /* 46 FileCpp       */ FILE_CPP,
    /* 47 FileCSharp    */ FILE_CSHARP,
    /* 48 FileGo        */ FILE_GO,
    /* 49 FileRust      */ FILE_RUST,
    /* 50 FileJs        */ FILE_JS,
    /* 51 FileTs        */ FILE_TS,
    /* 52 FileHtml      */ FILE_HTML,
    /* 53 FileCss       */ FILE_CSS,
    /* 54 FileJson      */ FILE_JSON,
    /* 55 FileYaml      */ FILE_YAML,
    /* 56 FileToml      */ FILE_TOML,
    /* 57 FileMarkdown  */ FILE_MARKDOWN,
    /* 58 FileShell     */ FILE_SHELL,
    /* 59 FileSql       */ FILE_SQL,
    /* 60 FileRuby      */ FILE_RUBY,
    /* 61 FilePhp       */ FILE_PHP,
    /* 62 FileLua       */ FILE_LUA,
    /* 63 FileSwift     */ FILE_SWIFT,
    /* 64 FileKotlin    */ FILE_KOTLIN,
    /* 65 FileDocker    */ FILE_DOCKER,
];
