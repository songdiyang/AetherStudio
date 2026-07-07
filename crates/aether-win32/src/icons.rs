//! 全局矢量图标系统（Direct2D PathGeometry 自绘，Lucide 风格简化版）
//!
//! 每个图标以 24x24 视口设计，渲染时按目标尺寸缩放。
//! 笔画宽度约 1.5（视口单位），用线条 + 圆角风格。
//! 所有 UI（欢迎页 / 状态栏 / 命令面板 / 活动栏）共用此缓存。

use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Factory, ID2D1HwndRenderTarget, ID2D1PathGeometry, ID2D1RenderTarget,
};

/// 图标标识
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconKind {
    // 欢迎页 / 文件操作
    OpenFolder,
    NewFile,
    Clone,
    Ssh,
    /// 关闭的文件夹（用于最近项目、文件树占位）
    Folder,
    /// 普通文件
    File,
    /// 保存（软盘）
    Save,
    // 编辑操作
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// 全选（复选框打勾）
    SelectAll,
    /// 查找（放大镜）
    Search,
    /// 替换（双向箭头）
    Replace,
    // 视图
    /// 侧边栏切换
    Sidebar,
    /// 左侧面板（活动栏）
    PanelLeft,
    /// 底部面板（状态栏）
    PanelBottom,
    // 转到
    /// 转到文件（带箭头的文件）
    GotoFile,
    /// 转到行（# 符号）
    Hash,
    // 运行 / 调试
    /// 播放（三角形）
    Play,
    /// 调试（虫子）
    Bug,
    // 终端
    Terminal,
    // 状态栏
    /// Git 分支
    GitBranch,
    /// 错误（圆圈带 X）
    Error,
    /// 警告（三角带 !）
    Warning,
    // 通用
    /// 信息（圆圈带 i）
    Info,
    /// 退出（门 + 箭头）
    Exit,
    // 导航
    /// 返回（左箭头）
    Back,
    /// 前进（右箭头）
    Forward,
    // 标题栏 / 通用操作
    /// 设置（齿轮）
    Settings,
    /// 用户（人形轮廓）
    User,
    /// 关闭（X 标记）
    Close,
    /// 加号（+ 标记）
    Plus,
    // 箭头 / 折角
    /// 左折角（<）
    ChevronLeft,
    /// 右折角（>）
    ChevronRight,
    // 品牌与 AI 助手
    /// 羊脸（欢迎页 logo）
    EmojiSheep,
    /// 机器人（AI 助手）
    Bot,
}

impl IconKind {
    /// 所有图标变体，用于一次性预创建几何
    pub const ALL: [IconKind; 38] = [
        IconKind::OpenFolder,
        IconKind::NewFile,
        IconKind::Clone,
        IconKind::Ssh,
        IconKind::Folder,
        IconKind::File,
        IconKind::Save,
        IconKind::Undo,
        IconKind::Redo,
        IconKind::Cut,
        IconKind::Copy,
        IconKind::Paste,
        IconKind::SelectAll,
        IconKind::Search,
        IconKind::Replace,
        IconKind::Sidebar,
        IconKind::PanelLeft,
        IconKind::PanelBottom,
        IconKind::GotoFile,
        IconKind::Hash,
        IconKind::Play,
        IconKind::Bug,
        IconKind::Terminal,
        IconKind::GitBranch,
        IconKind::Error,
        IconKind::Warning,
        IconKind::Info,
        IconKind::Exit,
        IconKind::Back,
        IconKind::Forward,
        IconKind::Settings,
        IconKind::User,
        IconKind::Close,
        IconKind::Plus,
        IconKind::ChevronLeft,
        IconKind::ChevronRight,
        IconKind::EmojiSheep,
        IconKind::Bot,
    ];
}

/// 构建并缓存所有图标几何（一次性创建，多次复用）
pub struct IconCache {
    geometries: Vec<(IconKind, ID2D1PathGeometry)>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            geometries: Vec::new(),
        }
    }

    /// 确保所有图标几何已创建（懒加载）。从 render target 获取 D2D factory。
    pub fn ensure_created_from_target(&mut self, target: &ID2D1HwndRenderTarget) {
        if !self.geometries.is_empty() {
            return;
        }
        let factory: Option<ID2D1Factory> = unsafe { target.GetFactory().ok() };
        let factory = match factory {
            Some(f) => f,
            None => return,
        };
        for kind in IconKind::ALL {
            if let Ok(geo) = build_icon(&factory, kind) {
                self.geometries.push((kind, geo));
            }
        }
    }

    /// P4-4: 设备丢失时清理所有缓存的几何对象，确保下次绘制时重建。
    /// PathGeometry 虽然绑定到 factory 而非 render target，但设备丢失场景下
    /// factory 本身可能也已失效，统一清理避免使用过期资源。
    pub fn clear(&mut self) {
        self.geometries.clear();
    }

    fn get(&self, kind: IconKind) -> Option<&ID2D1PathGeometry> {
        self.geometries
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, g)| g)
    }

    /// 在指定矩形内绘制图标（保持 24:24 比例居中）
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        kind: IconKind,
        rect_left: f32,
        rect_top: f32,
        rect_w: f32,
        rect_h: f32,
        brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) {
        let geo = match self.get(kind) {
            Some(g) => g,
            None => return,
        };
        unsafe {
            // 计算居中缩放：图标视口 24x24，缩放到目标尺寸（保留正方形比例）
            let scale = rect_w.min(rect_h) / 24.0;
            let tx = rect_left + (rect_w - 24.0 * scale) / 2.0;
            let ty = rect_top + (rect_h - 24.0 * scale) / 2.0;
            let transform = Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: tx,
                M32: ty,
            };
            let mut old = Matrix3x2::default();
            target.GetTransform(&mut old);
            target.SetTransform(&transform);
            target.DrawGeometry(geo, brush, 1.5, None);
            target.SetTransform(&old);
        }
    }
}

fn p(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

use windows::Win32::Graphics::Direct2D::Common::D2D1_BEZIER_SEGMENT;

fn bez(point1: D2D_POINT_2F, point2: D2D_POINT_2F, point3: D2D_POINT_2F) -> D2D1_BEZIER_SEGMENT {
    D2D1_BEZIER_SEGMENT {
        point1,
        point2,
        point3,
    }
}

/// 构建单个图标几何
fn build_icon(factory: &ID2D1Factory, kind: IconKind) -> windows::core::Result<ID2D1PathGeometry> {
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()? };
    unsafe {
        let sink = geo.Open()?;
        match kind {
            IconKind::OpenFolder => {
                // 文件夹打开图标：底部矩形 + 顶部折角 + 内部斜线表示打开
                sink.BeginFigure(p(3.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 7.0));
                sink.AddLine(p(11.0, 5.0));
                sink.AddLine(p(21.0, 5.0));
                sink.AddLine(p(21.0, 19.0));
                sink.AddLine(p(3.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 内部"打开"斜线
                sink.BeginFigure(p(3.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 7.0));
                sink.AddLine(p(21.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::NewFile => {
                // 新建文件：矩形 + 右上折角 + 中间加号
                sink.BeginFigure(p(6.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 3.0));
                sink.AddLine(p(19.0, 7.0));
                sink.AddLine(p(19.0, 21.0));
                sink.AddLine(p(6.0, 21.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(15.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 7.0));
                sink.AddLine(p(19.0, 7.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(9.0, 14.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 14.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.5, 10.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.5, 17.5));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Clone => {
                // Git 克隆：左侧实心圆 + 右侧两个空心圆 + 连接线（Git fork 风格）
                sink.BeginFigure(p(6.0, 18.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(3.5, 18.0), p(3.5, 14.0), p(6.0, 14.0)));
                sink.AddBezier(&bez(p(8.5, 14.0), p(8.5, 18.0), p(6.0, 18.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(18.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(15.5, 6.0), p(15.5, 2.0), p(18.0, 2.0)));
                sink.AddBezier(&bez(p(20.5, 2.0), p(20.5, 6.0), p(18.0, 6.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(18.0, 18.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(15.5, 18.0), p(15.5, 14.0), p(18.0, 14.0)));
                sink.AddBezier(&bez(p(20.5, 14.0), p(20.5, 18.0), p(18.0, 18.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(6.0, 14.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(18.0, 6.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(6.0, 14.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(18.0, 14.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Ssh => {
                // SSH/插头：左侧矩形插头 + 右侧两条引脚 + 圆形把手
                sink.BeginFigure(p(3.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 9.0));
                sink.AddLine(p(11.0, 15.0));
                sink.AddLine(p(3.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(11.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 11.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(11.0, 13.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 13.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(20.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(20.0, 9.0), p(15.0, 9.0), p(15.0, 12.0)));
                sink.AddBezier(&bez(p(15.0, 15.0), p(20.0, 15.0), p(20.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            IconKind::Folder => {
                // 关闭的文件夹：仅外轮廓（无打开斜线）
                sink.BeginFigure(p(3.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 7.0));
                sink.AddLine(p(11.0, 5.0));
                sink.AddLine(p(21.0, 5.0));
                sink.AddLine(p(21.0, 19.0));
                sink.AddLine(p(3.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            IconKind::File => {
                // 普通文件：矩形 + 右上折角
                sink.BeginFigure(p(6.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 3.0));
                sink.AddLine(p(19.0, 7.0));
                sink.AddLine(p(19.0, 21.0));
                sink.AddLine(p(6.0, 21.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(15.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 7.0));
                sink.AddLine(p(19.0, 7.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Save => {
                // 软盘：外框 + 顶部插槽 + 底部标签
                sink.BeginFigure(p(4.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(4.0, 20.0));
                sink.AddLine(p(20.0, 20.0));
                sink.AddLine(p(20.0, 4.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(8.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(8.0, 9.0));
                sink.AddLine(p(16.0, 9.0));
                sink.AddLine(p(16.0, 4.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(8.0, 13.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 13.0));
                sink.AddLine(p(16.0, 19.0));
                sink.AddLine(p(8.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Undo => {
                // 撤销：左箭头 + 弯曲尾部
                sink.BeginFigure(p(9.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(3.0, 9.0));
                sink.AddLine(p(9.0, 14.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(3.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 9.0));
                sink.AddBezier(&bez(p(20.0, 9.0), p(20.0, 19.0), p(14.0, 19.0)));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Redo => {
                // 重做：右箭头 + 弯曲尾部（Undo 的镜像）
                sink.BeginFigure(p(15.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 9.0));
                sink.AddLine(p(15.0, 14.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(21.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 9.0));
                sink.AddBezier(&bez(p(4.0, 9.0), p(4.0, 19.0), p(10.0, 19.0)));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Cut => {
                // 剪切：剪刀 — 两个圆 + 交叉线
                sink.BeginFigure(p(6.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(3.5, 6.0), p(3.5, 2.0), p(6.0, 2.0)));
                sink.AddBezier(&bez(p(8.5, 2.0), p(8.5, 6.0), p(6.0, 6.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(6.0, 18.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(3.5, 18.0), p(3.5, 14.0), p(6.0, 14.0)));
                sink.AddBezier(&bez(p(8.5, 14.0), p(8.5, 18.0), p(6.0, 18.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(8.0, 7.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 18.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(8.0, 16.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 6.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Copy => {
                // 复制：两个重叠矩形
                sink.BeginFigure(p(8.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 8.0));
                sink.AddLine(p(20.0, 20.0));
                sink.AddLine(p(8.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(4.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 4.0));
                sink.AddLine(p(16.0, 8.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(4.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(4.0, 16.0));
                sink.AddLine(p(8.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Paste => {
                // 粘贴：剪贴板 — 矩形 + 顶部夹子
                sink.BeginFigure(p(4.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(4.0, 20.0));
                sink.AddLine(p(20.0, 20.0));
                sink.AddLine(p(20.0, 8.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(8.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(8.0, 8.0));
                sink.AddLine(p(16.0, 8.0));
                sink.AddLine(p(16.0, 4.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(10.0, 2.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(14.0, 2.0));
                sink.AddLine(p(14.0, 6.0));
                sink.AddLine(p(10.0, 6.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::SelectAll => {
                // 全选：方框 + 勾
                sink.BeginFigure(p(3.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 3.0));
                sink.AddLine(p(21.0, 21.0));
                sink.AddLine(p(3.0, 21.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(7.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(10.5, 15.5));
                sink.AddLine(p(17.0, 9.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Search => {
                // 查找：放大镜 — 圆 + 把手
                sink.BeginFigure(p(11.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(4.0, 11.0), p(4.0, 4.0), p(11.0, 4.0)));
                sink.AddBezier(&bez(p(18.0, 4.0), p(18.0, 11.0), p(11.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(15.5, 15.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 21.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Replace => {
                // 替换：上箭头 + 下箭头（双向）
                sink.BeginFigure(p(7.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 4.0));
                sink.AddLine(p(15.0, 9.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(11.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(13.0, 15.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(17.0, 20.0));
                sink.AddLine(p(21.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(17.0, 20.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(17.0, 4.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Sidebar => {
                // 侧边栏：矩形 + 左侧分隔线
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 4.0));
                sink.AddLine(p(21.0, 20.0));
                sink.AddLine(p(3.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(9.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::PanelLeft => {
                // 左侧面板：矩形 + 左侧填充区 + 分隔
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 4.0));
                sink.AddLine(p(21.0, 20.0));
                sink.AddLine(p(3.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(9.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // 左侧两条短横线表示图标列
                sink.BeginFigure(p(5.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(7.0, 8.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(5.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(7.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(5.0, 16.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(7.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::PanelBottom => {
                // 底部面板：矩形 + 底部分隔
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 4.0));
                sink.AddLine(p(21.0, 20.0));
                sink.AddLine(p(3.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(3.0, 15.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::GotoFile => {
                // 转到文件：文件 + 右上箭头
                sink.BeginFigure(p(3.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(3.0, 21.0));
                sink.AddLine(p(17.0, 21.0));
                sink.AddLine(p(17.0, 13.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(9.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 3.0));
                sink.AddLine(p(21.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(21.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 13.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Hash => {
                // # 符号：两竖 + 两横
                sink.BeginFigure(p(8.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(8.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(16.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(4.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 9.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(4.0, 15.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Play => {
                // 播放：三角形指向右
                sink.BeginFigure(p(6.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 12.0));
                sink.AddLine(p(6.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            IconKind::Bug => {
                // 调试：虫子 — 椭圆身体 + 腿 + 触角
                sink.BeginFigure(p(12.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(8.0, 8.0), p(8.0, 4.0), p(12.0, 4.0)));
                sink.AddBezier(&bez(p(16.0, 4.0), p(16.0, 8.0), p(12.0, 8.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(8.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(8.0, 18.0), p(10.0, 20.0), p(12.0, 20.0)));
                sink.AddBezier(&bez(p(14.0, 20.0), p(16.0, 18.0), p(16.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(8.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(4.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(16.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(8.5, 16.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(5.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(15.5, 16.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(19.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Terminal => {
                // 终端：矩形 + > 提示符 + 光标下划线
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 4.0));
                sink.AddLine(p(21.0, 20.0));
                sink.AddLine(p(3.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(6.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(9.0, 12.0));
                sink.AddLine(p(6.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 15.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(18.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::GitBranch => {
                // Git 分支：两个圆点 + 连接线 + 分支
                // 上圆
                sink.BeginFigure(p(6.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(3.5, 5.0), p(3.5, 2.0), p(6.0, 2.0)));
                sink.AddBezier(&bez(p(8.5, 2.0), p(8.5, 5.0), p(6.0, 5.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 下圆
                sink.BeginFigure(p(6.0, 19.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(3.5, 19.0), p(3.5, 16.0), p(6.0, 16.0)));
                sink.AddBezier(&bez(p(8.5, 16.0), p(8.5, 19.0), p(6.0, 19.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 主干线
                sink.BeginFigure(p(6.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(6.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // 分支：从中间弯曲到右侧圆
                sink.BeginFigure(p(6.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(6.0, 8.0), p(18.0, 8.0), p(18.0, 11.0)));
                sink.AddLine(p(18.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // 右侧圆
                sink.BeginFigure(p(18.0, 19.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(15.5, 19.0), p(15.5, 16.0), p(18.0, 16.0)));
                sink.AddBezier(&bez(p(20.5, 16.0), p(20.5, 19.0), p(18.0, 19.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            IconKind::Error => {
                // 错误：圆圈 + X
                sink.BeginFigure(p(12.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(2.0, 12.0), p(2.0, 2.0), p(12.0, 2.0)));
                sink.AddBezier(&bez(p(22.0, 2.0), p(22.0, 12.0), p(12.0, 12.0)));
                sink.AddBezier(&bez(p(22.0, 12.0), p(22.0, 22.0), p(12.0, 22.0)));
                sink.AddBezier(&bez(p(2.0, 22.0), p(2.0, 12.0), p(12.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(8.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(16.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(8.0, 16.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Warning => {
                // 警告：三角形 + 感叹号
                sink.BeginFigure(p(12.0, 3.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(22.0, 20.0));
                sink.AddLine(p(2.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.BeginFigure(p(12.0, 9.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 14.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 17.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 17.5));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Info => {
                // 信息：圆圈 + i
                sink.BeginFigure(p(12.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(2.0, 12.0), p(2.0, 2.0), p(12.0, 2.0)));
                sink.AddBezier(&bez(p(22.0, 2.0), p(22.0, 12.0), p(12.0, 12.0)));
                sink.AddBezier(&bez(p(22.0, 12.0), p(22.0, 22.0), p(12.0, 22.0)));
                sink.AddBezier(&bez(p(2.0, 22.0), p(2.0, 12.0), p(12.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // i 点
                sink.BeginFigure(p(12.0, 7.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 8.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // i 竖
                sink.BeginFigure(p(12.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 17.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Exit => {
                // 退出：门 + 右箭头
                sink.BeginFigure(p(11.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(3.0, 20.0));
                sink.AddLine(p(11.0, 20.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(3.0, 4.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(11.0, 4.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(10.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(16.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 12.0));
                sink.AddLine(p(16.0, 17.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Back => {
                // 返回：左箭头 — 水平线 + 左指箭头
                sink.BeginFigure(p(3.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(9.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(3.0, 12.0));
                sink.AddLine(p(9.0, 18.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Forward => {
                // 前进：右箭头 — 水平线 + 右指箭头（Back 的镜像）
                sink.BeginFigure(p(3.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(15.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(21.0, 12.0));
                sink.AddLine(p(15.0, 18.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Settings => {
                // 设置：齿轮 — 外圆 + 内圆 + 8 个齿（向外短线）
                // 外圆（半径 7，圆心 12,12）
                sink.BeginFigure(p(12.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(5.0, 12.0), p(5.0, 5.0), p(12.0, 5.0)));
                sink.AddBezier(&bez(p(19.0, 5.0), p(19.0, 12.0), p(12.0, 12.0)));
                sink.AddBezier(&bez(p(19.0, 12.0), p(19.0, 19.0), p(12.0, 19.0)));
                sink.AddBezier(&bez(p(5.0, 19.0), p(5.0, 12.0), p(12.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 内圆（半径 3，圆心 12,12）
                sink.BeginFigure(p(12.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(9.0, 12.0), p(9.0, 9.0), p(12.0, 9.0)));
                sink.AddBezier(&bez(p(15.0, 9.0), p(15.0, 12.0), p(12.0, 12.0)));
                sink.AddBezier(&bez(p(15.0, 12.0), p(15.0, 15.0), p(12.0, 15.0)));
                sink.AddBezier(&bez(p(9.0, 15.0), p(9.0, 12.0), p(12.0, 12.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 8 个齿：从外圆向外延伸的短线（4 正方向 + 4 对角）
                sink.BeginFigure(p(19.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(22.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 19.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 22.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(5.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(2.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 2.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(17.0, 17.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(19.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(7.0, 17.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(5.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(7.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(5.0, 5.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(17.0, 7.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(19.0, 5.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::User => {
                // 用户：人形轮廓 — 头部圆形 + 肩部弧线
                // 头部圆（半径 3.5，圆心 12,8）
                sink.BeginFigure(p(12.0, 8.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(8.5, 8.0), p(8.5, 4.5), p(12.0, 4.5)));
                sink.AddBezier(&bez(p(15.5, 4.5), p(15.5, 8.0), p(12.0, 8.0)));
                sink.AddBezier(&bez(p(15.5, 8.0), p(15.5, 11.5), p(12.0, 11.5)));
                sink.AddBezier(&bez(p(8.5, 11.5), p(8.5, 8.0), p(12.0, 8.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 肩部弧线：从 (5,21) 向上弯曲到 (19,21)
                sink.BeginFigure(p(5.0, 21.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(5.0, 15.0), p(19.0, 15.0), p(19.0, 21.0)));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Close => {
                // 关闭：X 标记 — 两条对角线
                sink.BeginFigure(p(5.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(19.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(19.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(5.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Plus => {
                // 加号：水平线 + 垂直线
                sink.BeginFigure(p(5.0, 12.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(19.0, 12.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.BeginFigure(p(12.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(12.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::ChevronLeft => {
                // 左折角：单条 < 折线
                sink.BeginFigure(p(16.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(8.0, 12.0));
                sink.AddLine(p(16.0, 18.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::ChevronRight => {
                // 右折角：单条 > 折线（ChevronLeft 的镜像）
                sink.BeginFigure(p(8.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(16.0, 12.0));
                sink.AddLine(p(8.0, 18.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::EmojiSheep => {
                // 羊脸（欢迎页 logo）：头部大圆 + 两耳 + 两眼 + 顶部小绒毛
                // 头部圆（半径 7，圆心 12,13）
                sink.BeginFigure(p(12.0, 13.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(5.0, 13.0), p(5.0, 6.0), p(12.0, 6.0)));
                sink.AddBezier(&bez(p(19.0, 6.0), p(19.0, 13.0), p(12.0, 13.0)));
                sink.AddBezier(&bez(p(19.0, 13.0), p(19.0, 20.0), p(12.0, 20.0)));
                sink.AddBezier(&bez(p(5.0, 20.0), p(5.0, 13.0), p(12.0, 13.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 左耳（半径 2，圆心 4,11）
                sink.BeginFigure(p(4.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(2.0, 11.0), p(2.0, 9.0), p(4.0, 9.0)));
                sink.AddBezier(&bez(p(6.0, 9.0), p(6.0, 11.0), p(4.0, 11.0)));
                sink.AddBezier(&bez(p(6.0, 11.0), p(6.0, 13.0), p(4.0, 13.0)));
                sink.AddBezier(&bez(p(2.0, 13.0), p(2.0, 11.0), p(4.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 右耳（半径 2，圆心 20,11）
                sink.BeginFigure(p(20.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(18.0, 11.0), p(18.0, 9.0), p(20.0, 9.0)));
                sink.AddBezier(&bez(p(22.0, 9.0), p(22.0, 11.0), p(20.0, 11.0)));
                sink.AddBezier(&bez(p(22.0, 11.0), p(22.0, 13.0), p(20.0, 13.0)));
                sink.AddBezier(&bez(p(18.0, 13.0), p(18.0, 11.0), p(20.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 左眼（实心小圆，半径 0.8，圆心 9,11）
                sink.BeginFigure(p(9.0, 11.0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&bez(p(8.2, 11.0), p(8.2, 10.2), p(9.0, 10.2)));
                sink.AddBezier(&bez(p(9.8, 10.2), p(9.8, 11.0), p(9.0, 11.0)));
                sink.AddBezier(&bez(p(9.8, 11.0), p(9.8, 11.8), p(9.0, 11.8)));
                sink.AddBezier(&bez(p(8.2, 11.8), p(8.2, 11.0), p(9.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 右眼（实心小圆，半径 0.8，圆心 15,11）
                sink.BeginFigure(p(15.0, 11.0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&bez(p(14.2, 11.0), p(14.2, 10.2), p(15.0, 10.2)));
                sink.AddBezier(&bez(p(15.8, 10.2), p(15.8, 11.0), p(15.0, 11.0)));
                sink.AddBezier(&bez(p(15.8, 11.0), p(15.8, 11.8), p(15.0, 11.8)));
                sink.AddBezier(&bez(p(14.2, 11.8), p(14.2, 11.0), p(15.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 顶部小绒毛弧线
                sink.BeginFigure(p(10.0, 6.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(10.0, 3.0), p(14.0, 3.0), p(14.0, 6.0)));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            IconKind::Bot => {
                // 机器人（AI 助手）：头部矩形 + 两天线 + 两眼 + 嘴巴
                // 头部矩形 (4,5)-(20,19)
                sink.BeginFigure(p(4.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(20.0, 5.0));
                sink.AddLine(p(20.0, 19.0));
                sink.AddLine(p(4.0, 19.0));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 左天线
                sink.BeginFigure(p(10.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(10.0, 2.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // 右天线
                sink.BeginFigure(p(14.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(14.0, 2.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                // 左天线顶部小圆（半径 1，圆心 10,2）
                sink.BeginFigure(p(10.0, 2.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(9.0, 2.0), p(9.0, 1.0), p(10.0, 1.0)));
                sink.AddBezier(&bez(p(11.0, 1.0), p(11.0, 2.0), p(10.0, 2.0)));
                sink.AddBezier(&bez(p(11.0, 2.0), p(11.0, 3.0), p(10.0, 3.0)));
                sink.AddBezier(&bez(p(9.0, 3.0), p(9.0, 2.0), p(10.0, 2.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 右天线顶部小圆（半径 1，圆心 14,2）
                sink.BeginFigure(p(14.0, 2.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddBezier(&bez(p(13.0, 2.0), p(13.0, 1.0), p(14.0, 1.0)));
                sink.AddBezier(&bez(p(15.0, 1.0), p(15.0, 2.0), p(14.0, 2.0)));
                sink.AddBezier(&bez(p(15.0, 2.0), p(15.0, 3.0), p(14.0, 3.0)));
                sink.AddBezier(&bez(p(13.0, 3.0), p(13.0, 2.0), p(14.0, 2.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 左眼（实心小圆，半径 1.5，圆心 9,11）
                sink.BeginFigure(p(9.0, 11.0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&bez(p(7.5, 11.0), p(7.5, 9.5), p(9.0, 9.5)));
                sink.AddBezier(&bez(p(10.5, 9.5), p(10.5, 11.0), p(9.0, 11.0)));
                sink.AddBezier(&bez(p(10.5, 11.0), p(10.5, 12.5), p(9.0, 12.5)));
                sink.AddBezier(&bez(p(7.5, 12.5), p(7.5, 11.0), p(9.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 右眼（实心小圆，半径 1.5，圆心 15,11）
                sink.BeginFigure(p(15.0, 11.0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&bez(p(13.5, 11.0), p(13.5, 9.5), p(15.0, 9.5)));
                sink.AddBezier(&bez(p(16.5, 9.5), p(16.5, 11.0), p(15.0, 11.0)));
                sink.AddBezier(&bez(p(16.5, 11.0), p(16.5, 12.5), p(15.0, 12.5)));
                sink.AddBezier(&bez(p(13.5, 12.5), p(13.5, 11.0), p(15.0, 11.0)));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                // 嘴巴：水平线
                sink.BeginFigure(p(9.0, 15.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p(15.0, 15.0));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
        }
        sink.Close()?;
    }
    Ok(geo)
}

/// 创建笔画样式（圆角端点）
#[allow(dead_code)]
fn create_round_stroke(
    factory: &ID2D1Factory,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle> {
    unsafe {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_LINE_JOIN_ROUND,
            D2D1_STROKE_STYLE_PROPERTIES,
        };
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 1.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
        };
        factory.CreateStrokeStyle(&props, None)
    }
}

#[cfg(test)]
mod tests {
    use super::IconKind;

    /// 验证 ALL 数组长度为 38（28 原有 + 10 新增）
    #[test]
    fn all_icons_count_is_38() {
        assert_eq!(IconKind::ALL.len(), 38);
    }

    /// 验证每个新增变体都存在于 ALL 数组中
    #[test]
    fn new_variants_present_in_all() {
        let new_kinds = [
            IconKind::Back,
            IconKind::Forward,
            IconKind::Settings,
            IconKind::User,
            IconKind::Close,
            IconKind::Plus,
            IconKind::ChevronLeft,
            IconKind::ChevronRight,
            IconKind::EmojiSheep,
            IconKind::Bot,
        ];
        for kind in new_kinds {
            assert!(
                IconKind::ALL.iter().any(|k| *k == kind),
                "新增图标 {:?} 未在 ALL 数组中找到",
                kind
            );
        }
    }

    /// 验证 ALL 数组中无重复项
    #[test]
    fn all_icons_no_duplicates() {
        for i in 0..IconKind::ALL.len() {
            for j in (i + 1)..IconKind::ALL.len() {
                assert_ne!(
                    IconKind::ALL[i], IconKind::ALL[j],
                    "ALL 数组中存在重复项：索引 {} 与 {} 均为 {:?}",
                    i, j, IconKind::ALL[i]
                );
            }
        }
    }
}
