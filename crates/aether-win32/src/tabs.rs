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
            language,
        })
    }

    pub fn rebuild_cache(&mut self) {
        let total_lines = self.buffer.len_lines().max(1);
        let lexer = self.language.create_lexer();
        if self.cached_lines.len() != total_lines {
            self.cached_lines.resize_with(total_lines, || String::new());
            self.cached_tokens.resize_with(total_lines, || Vec::new());
            self.line_cache_versions.resize(total_lines, 0);
        }
        for i in 0..total_lines {
            if self.line_cache_versions[i] != self.buffer_version {
                let line = self.buffer.get_line(i).unwrap_or_default();
                let tokens = lexer.lex_full(&line);
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
