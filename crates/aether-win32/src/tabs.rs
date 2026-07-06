use std::path::PathBuf;

use aether_core::buffer::history::History;
use aether_core::buffer::piece_table::PieceTable;
use aether_core::lexer::{Language, LexemeSpan};

/// 标签页 - 包含完整的文件编辑状态
pub struct Tab {
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
    // 渲染缓存（同crate内可访问）
    pub(crate) cached_lines: Vec<String>,
    pub(crate) cached_tokens: Vec<Vec<LexemeSpan>>,
    pub(crate) line_cache_versions: Vec<u64>,
    pub(crate) buffer_version: u64,
    /// P2.3: 大文件标记
    pub(crate) is_large_file: bool,
    /// P2.3: 行 Y 偏移前缀和缓存
    pub(crate) line_y_offsets: Vec<f32>,
    /// P3.1: 当前内联补全建议
    pub(crate) inline_completion: Option<crate::inline_completion::InlineCompletion>,
    // 语言类型
    pub(crate) language: Language,
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

impl Tab {
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
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version: 0,
            is_large_file: false,
            line_y_offsets: Vec::new(),
            inline_completion: None,
            language: Language::PlainText,
        }
    }

    pub fn from_file(path: PathBuf) -> std::io::Result<Self> {
        let buffer = PieceTable::from_file(&path)?;
        let language = Language::from_path(&path);
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
            cached_lines: Vec::new(),
            cached_tokens: Vec::new(),
            line_cache_versions: Vec::new(),
            buffer_version: 1,
            is_large_file: false,
            line_y_offsets: Vec::new(),
            inline_completion: None,
            language,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_new_is_unnamed() {
        let tab = Tab::new();
        assert!(tab.file_path.is_none());
        assert_eq!(tab.file_name(), "未命名");
        assert_eq!(tab.cursor_line, 0);
        assert!(!tab.is_dirty);
    }

    #[test]
    fn test_tab_file_name() {
        let mut tab = Tab::new();
        tab.file_path = Some(PathBuf::from("D:\\project\\src\\main.rs"));
        assert_eq!(tab.file_name(), "main.rs");
    }

    #[test]
    fn test_tab_mark_dirty() {
        let mut tab = Tab::new();
        assert_eq!(tab.buffer_version, 0);
        tab.mark_dirty();
        assert!(tab.is_dirty);
        assert_eq!(tab.buffer_version, 1);
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
        assert_eq!(tab.file_path, Some(path));
        assert_eq!(tab.file_name(), "sample.rs");
        assert_eq!(tab.language, Language::Rust);
        assert_eq!(tab.buffer_version, 1);

        tab.rebuild_cache();
        assert!(!tab.cached_lines.is_empty());
        assert_eq!(tab.cached_lines[0], "fn main() {}");
        assert!(!tab.cached_tokens.is_empty());

        tab.clear_dirty();
        assert!(!tab.is_dirty);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tab_file_name_fallback() {
        let mut tab = Tab::new();
        tab.file_path = Some(PathBuf::from("/"));
        assert_eq!(tab.file_name(), "未命名");
    }

    #[test]
    fn test_tab_mark_dirty_increments_version() {
        let mut tab = Tab::new();
        let v0 = tab.buffer_version;
        tab.mark_dirty();
        assert!(tab.is_dirty);
        assert_eq!(tab.buffer_version, v0 + 1);
    }
}
