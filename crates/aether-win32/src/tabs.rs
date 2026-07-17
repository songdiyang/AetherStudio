use std::path::PathBuf;

use aether_core::buffer::history::History;
use aether_core::buffer::piece_table::PieceTable;
use aether_core::lexer::{Language, LexemeSpan};

/// 标签页内容状态 — 包含所有 per-tab 的编辑状态
///
/// REQ-P1-09: 将 Tab 和 EditorState 中重复的 per-tab 字段统一到此处，
/// 通过 std::mem::swap 实现标签切换，消除手动字段同步。
#[derive(Clone, Debug)]
pub struct TabContent {
    pub file_path: Option<PathBuf>,
    pub buffer: PieceTable,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
    pub scroll_y: f32,
    /// P0-3: 水平滚动偏移（与 EditorState.scroll_x 同步）
    pub scroll_x: f32,
    pub history: History,
    pub is_dirty: bool,
    /// 自动保存：上次成功落盘时对应的 buffer_version，用于内容去重（跳过无变化保存）
    pub last_saved_buffer_version: u64,
    /// 自动保存：上次已知文件 mtime，用于检测外部修改（mtime 轮询冲突检测）
    pub last_known_mtime: Option<std::time::SystemTime>,
    /// 自动保存：检测到外部修改后置位，暂停自动保存；手动保存后复位
    pub auto_save_conflict: bool,
    // 渲染缓存（同crate内可访问）
    pub(crate) cached_lines: Vec<String>,
    pub(crate) cached_tokens: Vec<Vec<LexemeSpan>>,
    pub(crate) line_cache_versions: Vec<u64>,
    pub(crate) buffer_version: u64,
    /// REQ-P2-01: 上次 rebuild_cache 的签名，用于跳过无变化的缓存重建
    pub(crate) last_cache_signature: (u64, usize, usize, usize),
    /// P2.3: 大文件标记
    pub(crate) is_large_file: bool,
    /// P2.3: 行 Y 偏移前缀和缓存
    pub(crate) line_y_offsets: Vec<f32>,
    /// P3.1: 当前内联补全建议
    pub(crate) inline_completion: Option<crate::inline_completion::InlineCompletion>,
    // 语言类型
    pub(crate) language: Language,
}

impl TabContent {
    pub fn new() -> Self {
        Self {
            file_path: None,
            buffer: PieceTable::from_string(String::new()),
            cursor_line: 0,
            cursor_col: 0,
            selection_start: None,
            selection_end: None,
            scroll_y: 0.0,
            scroll_x: 0.0,
            history: History::new(),
            is_dirty: false,
            last_saved_buffer_version: 0,
            last_known_mtime: None,
            auto_save_conflict: false,
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version: 0,
            last_cache_signature: (0, 0, 0, 0),
            is_large_file: false,
            line_y_offsets: Vec::new(),
            inline_completion: None,
            language: Language::PlainText,
        }
    }

    pub fn from_file(path: PathBuf) -> std::io::Result<Self> {
        let buffer = PieceTable::from_file(&path)?;
        let language = Language::from_path(&path);
        // 自动保存：记录文件加载时的 mtime，作为后续外部修改检测的基线
        let last_known_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        // 加载时内容与磁盘一致：last_saved_buffer_version 对齐当前 buffer_version
        Ok(Self {
            file_path: Some(path.clone()),
            buffer,
            cursor_line: 0,
            cursor_col: 0,
            selection_start: None,
            selection_end: None,
            scroll_y: 0.0,
            scroll_x: 0.0,
            history: History::new(),
            is_dirty: false,
            last_saved_buffer_version: 1,
            last_known_mtime,
            auto_save_conflict: false,
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version: 1,
            last_cache_signature: (0, 0, 0, 0),
            is_large_file: false,
            line_y_offsets: Vec::new(),
            inline_completion: None,
            language,
        })
    }

    /// 从已加载的缓冲区构造 TabContent，用于非 `from_file` 的加载路径
    /// （图片 / 差异视图 / 远程文件 / 新建文件等）。
    ///
    /// 统一这些路径的字段初始化，消除重复（REQ-P1-09）。自动保存状态：
    /// - `last_saved_buffer_version`：已加载内容与"源"一致时对齐 buffer_version（去重跳过）；
    ///   新建文件（`is_dirty=true`）置 0，使首次自动保存能触发。
    /// - `last_known_mtime`：None（首次保存成功后由 `note_save_succeeded` 建立基线）。
    pub fn with_loaded_buffer(
        file_path: Option<PathBuf>,
        buffer: PieceTable,
        language: Language,
        is_dirty: bool,
    ) -> Self {
        let buffer_version: u64 = 1;
        Self {
            file_path,
            buffer,
            cursor_line: 0,
            cursor_col: 0,
            selection_start: None,
            selection_end: None,
            scroll_y: 0.0,
            scroll_x: 0.0,
            history: History::new(),
            is_dirty,
            last_saved_buffer_version: if is_dirty { 0 } else { buffer_version },
            last_known_mtime: None,
            auto_save_conflict: false,
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version,
            last_cache_signature: (0, 0, 0, 0),
            is_large_file: false,
            line_y_offsets: Vec::new(),
            inline_completion: None,
            language,
        }
    }

    pub fn rebuild_cache(&mut self) {
        let total_lines = self.buffer.len_lines().max(1);
        let lang = self.language;
        if self.cached_lines.len() != total_lines {
            self.cached_lines.resize_with(total_lines, String::new);
            self.cached_tokens.resize_with(total_lines, Vec::new);
            self.line_cache_versions.resize(total_lines, 0);
        }
        for i in 0..total_lines {
            if self.line_cache_versions[i] != self.buffer_version {
                let line = self.buffer.get_line(i).unwrap_or_default();
                let tokens = lang.lex_full(&line);
                self.cached_lines[i] = line;
                self.cached_tokens[i] = tokens;
                self.line_cache_versions[i] = self.buffer_version;
            }
        }
    }

    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
        self.buffer_version += 1;
    }

    pub fn clear_dirty(&mut self) {
        self.is_dirty = false;
    }

    pub fn file_name(&self) -> String {
        match &self.file_path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string()),
            None => "未命名".to_string(),
        }
    }
}

/// 标签页类型 — 支持文件、设置、欢迎三种标签页
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Tab {
    /// 文件编辑标签页
    File(TabContent),
    /// 设置面板标签页
    Settings,
    /// 欢迎页标签页
    Welcome,
}

impl Tab {
    /// 创建新的文件标签页（从文件路径加载）
    pub fn from_file(path: PathBuf) -> std::io::Result<Self> {
        Ok(Tab::File(TabContent::from_file(path)?))
    }

    /// 创建新的空文件标签页（向后兼容：旧代码期望 new() 返回可保存内容的标签）
    pub fn new() -> Self {
        Tab::File(TabContent::new())
    }

    /// 判断是否为文件标签页
    pub fn is_file(&self) -> bool {
        matches!(self, Tab::File(_))
    }

    /// 判断是否为设置标签页
    pub fn is_settings(&self) -> bool {
        matches!(self, Tab::Settings)
    }

    /// 判断是否为欢迎标签页
    pub fn is_welcome(&self) -> bool {
        matches!(self, Tab::Welcome)
    }

    /// 获取文件路径（仅 File 类型）
    pub fn file_path(&self) -> Option<&PathBuf> {
        match self {
            Tab::File(content) => content.file_path.as_ref(),
            _ => None,
        }
    }

    /// 获取标签页标题
    pub fn title(&self) -> String {
        match self {
            Tab::File(content) => content.file_name(),
            Tab::Settings => "设置".to_string(),
            Tab::Welcome => "欢迎".to_string(),
        }
    }

    /// 判断文件标签页是否已修改
    pub fn is_dirty(&self) -> bool {
        match self {
            Tab::File(content) => content.is_dirty,
            _ => false,
        }
    }

    /// 获取文件标签页的可变内容引用
    pub fn as_file_mut(&mut self) -> Option<&mut TabContent> {
        match self {
            Tab::File(content) => Some(content),
            _ => None,
        }
    }

    /// 获取文件标签页的不可变内容引用
    pub fn as_file(&self) -> Option<&TabContent> {
        match self {
            Tab::File(content) => Some(content),
            _ => None,
        }
    }

    pub fn rebuild_cache(&mut self) {
        if let Tab::File(content) = self {
            content.rebuild_cache();
        }
    }

    pub fn mark_dirty(&mut self) {
        if let Tab::File(content) = self {
            content.mark_dirty();
        }
    }

    pub fn clear_dirty(&mut self) {
        if let Tab::File(content) = self {
            content.clear_dirty();
        }
    }

    pub fn file_name(&self) -> String {
        match self {
            Tab::File(content) => content.file_name(),
            Tab::Settings => "设置".to_string(),
            Tab::Welcome => "欢迎".to_string(),
        }
    }
}

/// 标签栏布局信息（用于点击检测）
#[derive(Clone, Debug)]
pub(crate) struct TabLayout {
    pub(crate) index: usize,
    pub(crate) x: f32,
    pub(crate) width: f32,
    pub(crate) close_x: f32,
    pub(crate) close_width: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_new_is_welcome() {
        let tab = Tab::new();
        // Tab::new() 现在返回 File 变体以保持向后兼容（remove_tab_saving_content 等逻辑需要）
        assert!(tab.is_file());
        assert!(!tab.is_welcome());
        assert!(!tab.is_settings());
        assert_eq!(tab.title(), "未命名");
    }

    #[test]
    fn test_tab_file_name() {
        let mut content = TabContent::new();
        content.file_path = Some(PathBuf::from("D:\\project\\src\\main.rs"));
        let tab = Tab::File(content);
        assert_eq!(tab.file_name(), "main.rs");
        assert!(tab.is_file());
        assert_eq!(tab.title(), "main.rs");
    }

    #[test]
    fn test_tab_settings() {
        let tab = Tab::Settings;
        assert!(tab.is_settings());
        assert_eq!(tab.title(), "设置");
        assert!(!tab.is_dirty());
    }

    #[test]
    fn test_tab_mark_dirty() {
        let content = TabContent::new();
        let mut tab = Tab::File(content);
        assert_eq!(tab.as_file().unwrap().buffer_version, 0);
        tab.mark_dirty();
        assert!(tab.is_dirty());
        assert_eq!(tab.as_file().unwrap().buffer_version, 1);
    }

    #[test]
    fn test_tab_layout_fields() {
        let layout = TabLayout {
            index: 0,
            x: 10.0,
            width: 120.0,
            close_x: 100.0,
            close_width: 18.0,
        };
        assert_eq!(layout.close_x + layout.close_width, 118.0);
    }

    #[test]
    fn test_tab_from_file_and_rebuild_cache() {
        let dir = std::env::temp_dir().join(format!("aether_tab_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();

        let mut tab = Tab::from_file(path.clone()).unwrap();
        assert_eq!(tab.file_path(), Some(&path));
        assert_eq!(tab.file_name(), "sample.rs");
        assert!(tab.is_file());

        tab.rebuild_cache();
        assert!(!tab.as_file().unwrap().cached_lines.is_empty());
        assert_eq!(tab.as_file().unwrap().cached_lines[0], "fn main() {}");

        tab.clear_dirty();
        assert!(!tab.is_dirty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tab_file_name_fallback() {
        let mut content = TabContent::new();
        content.file_path = Some(PathBuf::from("/"));
        let tab = Tab::File(content);
        assert_eq!(tab.file_name(), "未命名");
    }

    #[test]
    fn test_tab_mark_dirty_increments_version() {
        let content = TabContent::new();
        let mut tab = Tab::File(content);
        let v0 = tab.as_file().unwrap().buffer_version;
        tab.mark_dirty();
        assert!(tab.is_dirty());
        assert_eq!(tab.as_file().unwrap().buffer_version, v0 + 1);
    }
}
