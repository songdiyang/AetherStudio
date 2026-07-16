//! 全局矢量图标系统（基于开源 SVG：Lucide ISC + Devicon MIT）
//!
//! 所有图标均嵌入为 24x24 viewBox 的 SVG 形状（path/circle/rect/line/polygon）。
//! 渲染时按目标尺寸缩放。
//!
//! 数据来源：
//! - Lucide Icons (ISC, https://lucide.dev) — UI 图标
//! - Devicon (MIT, https://devicon.dev) — 文件类型图标设计参考
//!
//! 商业友好（无需署名）。

use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Direct2D::ID2D1PathGeometry;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;

use super::icons_svg::build_def;
use super::icons_svg_defs::SVG_DEFS;

/// 图标标识
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum IconKind {
    // 欢迎页 / 文件操作
    OpenFolder,
    NewFile,
    Clone,
    Ssh,
    /// 关闭的文件夹
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
    /// 全选
    SelectAll,
    /// 查找
    Search,
    /// 替换
    Replace,
    // 视图
    Sidebar,
    PanelLeft,
    PanelBottom,
    // 转到
    GotoFile,
    Hash,
    // 运行 / 调试
    Play,
    Bug,
    // 终端
    Terminal,
    // 状态栏
    GitBranch,
    /// 错误
    Error,
    /// 警告
    Warning,
    /// 信息
    Info,
    /// 退出
    Exit,
    // 导航
    Back,
    Forward,
    // 标题栏 / 通用操作
    Settings,
    User,
    Close,
    Plus,
    ChevronLeft,
    ChevronRight,
    // 品牌与 AI 助手
    EmojiSheep,
    Bot,
    // AI 面板输入框工具栏
    Send,
    Mic,
    Sparkles,
    List,
    // 文件类型
    FilePython,
    FileJava,
    FileText,
    FileC,
    FileCpp,
    FileCSharp,
    FileGo,
    FileRust,
    FileJs,
    FileTs,
    FileHtml,
    FileCss,
    FileJson,
    FileYaml,
    FileToml,
    FileMarkdown,
    FileShell,
    FileSql,
    FileRuby,
    FilePhp,
    FileLua,
    FileSwift,
    FileKotlin,
    FileDocker,
}

impl IconKind {
    /// 所有图标变体索引（与 SVG_DEFS 数组下标对应）
    pub const ALL: [IconKind; 66] = [
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
        IconKind::Send,
        IconKind::Mic,
        IconKind::Sparkles,
        IconKind::List,
        IconKind::FilePython,
        IconKind::FileJava,
        IconKind::FileText,
        IconKind::FileC,
        IconKind::FileCpp,
        IconKind::FileCSharp,
        IconKind::FileGo,
        IconKind::FileRust,
        IconKind::FileJs,
        IconKind::FileTs,
        IconKind::FileHtml,
        IconKind::FileCss,
        IconKind::FileJson,
        IconKind::FileYaml,
        IconKind::FileToml,
        IconKind::FileMarkdown,
        IconKind::FileShell,
        IconKind::FileSql,
        IconKind::FileRuby,
        IconKind::FilePhp,
        IconKind::FileLua,
        IconKind::FileSwift,
        IconKind::FileKotlin,
        IconKind::FileDocker,
    ];

    /// 索引到 SVG_DEFS 数组下标
    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|k| k == self).unwrap_or(0)
    }
}

/// 单个图标渲染项：(geometry, fill_color)
/// fill_color = None 表示描边（用当前 brush）
/// fill_color = Some(rgba) 表示填充（用此颜色，覆盖 brush）
type IconLayer = (ID2D1PathGeometry, Option<(f32, f32, f32, f32)>);

/// 构建并缓存所有图标几何（一次性创建，多次复用）
pub struct IconCache {
    /// 已构建的图标 (SvgDef 引用 → 渲染层列表)
    layers: Vec<IconLayer>,
    /// 已构建的索引 (IconKind 索引 → 起始层)
    index_offsets: Vec<(usize, usize)>, // (start, len)
    /// viewBox 缓存（用于渲染时缩放）
    viewboxes: Vec<(f32, f32, f32, f32)>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            index_offsets: Vec::new(),
            viewboxes: Vec::new(),
        }
    }

    /// 确保所有图标几何已创建（懒加载）。从 render target 获取 D2D factory。
    pub fn ensure_created_from_target(&mut self, target: &ID2D1HwndRenderTarget) {
        if !self.index_offsets.is_empty() {
            return;
        }
        let factory: Option<windows::Win32::Graphics::Direct2D::ID2D1Factory> =
            unsafe { target.GetFactory().ok() };
        let factory = match factory {
            Some(f) => f,
            None => return,
        };
        for def in SVG_DEFS {
            let vb = def.viewbox;
            let start = self.layers.len();
            let new_layers = build_def(&factory, def);
            let len = new_layers.len();
            self.layers.extend(new_layers);
            self.index_offsets.push((start, len));
            self.viewboxes.push(vb);
        }
    }

    /// 设备丢失时清理所有缓存
    pub fn clear(&mut self) {
        self.layers.clear();
        self.index_offsets.clear();
        self.viewboxes.clear();
    }

    /// 在指定矩形内绘制图标（保持 viewBox 比例居中）
    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        kind: IconKind,
        rect_left: f32,
        rect_top: f32,
        rect_w: f32,
        rect_h: f32,
        brush: &ID2D1SolidColorBrush,
    ) {
        let idx = kind.index();
        let (start, len) = match self.index_offsets.get(idx) {
            Some(&v) => v,
            None => return,
        };
        if len == 0 {
            return;
        }
        let (vb_x, vb_y, vb_w, vb_h) = self.viewboxes[idx];
        // 缩放到目标矩形（保持 viewBox 比例，居中）
        let scale = (rect_w / vb_w).min(rect_h / vb_h);
        let draw_w = vb_w * scale;
        let draw_h = vb_h * scale;
        let tx = rect_left + (rect_w - draw_w) / 2.0 - vb_x * scale;
        let ty = rect_top + (rect_h - draw_h) / 2.0 - vb_y * scale;

        let transform = Matrix3x2 {
            M11: scale,
            M12: 0.0,
            M21: 0.0,
            M22: scale,
            M31: tx,
            M32: ty,
        };

        unsafe {
            let mut old = Matrix3x2::default();
            target.GetTransform(&mut old);
            target.SetTransform(&transform);

            // 第一遍：填充层（fill = Some 的几何）
            for (geo, fill) in &self.layers[start..start + len] {
                if let Some((r, g, b, a)) = fill {
                    let color = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                        r: *r,
                        g: *g,
                        b: *b,
                        a: *a,
                    };
                    if let Ok(layer_brush) = target.CreateSolidColorBrush(&color, None) {
                        target.FillGeometry(geo, &layer_brush, None);
                    }
                }
            }
            // 第二遍：描边层（fill = None 的几何），用传入 brush 描边
            // 笔画宽度根据缩放调整（1.5 在 24x24 视口 ≈ 1.5 * scale 像素）
            let stroke_w = 1.5 * scale;
            for (geo, fill) in &self.layers[start..start + len] {
                if fill.is_none() {
                    target.DrawGeometry(geo, brush, stroke_w, None);
                }
            }

            target.SetTransform(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IconKind;

    /// 验证 ALL 数组长度为 66
    #[test]
    fn all_icons_count_is_66() {
        assert_eq!(IconKind::ALL.len(), 66);
    }

    /// 验证 ALL 数组中无重复项
    #[test]
    fn all_icons_no_duplicates() {
        for i in 0..IconKind::ALL.len() {
            for j in (i + 1)..IconKind::ALL.len() {
                assert_ne!(
                    IconKind::ALL[i],
                    IconKind::ALL[j],
                    "ALL 数组中存在重复项：索引 {} 与 {} 均为 {:?}",
                    i,
                    j,
                    IconKind::ALL[i]
                );
            }
        }
    }

    /// 验证每个 IconKind 都对应一个 SVG_DEFS
    #[test]
    fn icon_kind_index_in_bounds() {
        use super::super::icons_svg_defs::SVG_DEFS;
        for &kind in IconKind::ALL.iter() {
            let idx = kind.index();
            assert!(
                idx < SVG_DEFS.len(),
                "IconKind::{:?} 索引 {} 超出 SVG_DEFS 范围",
                kind,
                idx
            );
        }
    }
}
