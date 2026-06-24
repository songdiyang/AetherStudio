use crate::buffer::text_buffer::EditResult;
use crate::lexer::{Language, LexemeSpan};

/// 增量词法分析器
///
/// 缓存每行的token结果，只在编辑时重新分析受影响的行
/// 使用 Vec 存储（行号 = 索引），O(1) 访问，连续内存布局
pub struct IncrementalLexer {
    language: Language,
    /// 每行缓存的token（行索引直接作为 Vec 索引）
    line_tokens: Vec<Vec<LexemeSpan>>,
    /// 缓存版本号（用于失效检测）
    version: u64,
    /// 总行数（上次分析时的）
    last_line_count: usize,
}

impl IncrementalLexer {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            line_tokens: Vec::new(),
            version: 0,
            last_line_count: 0,
        }
    }

    /// 全量分析所有行（首次打开文件时使用）
    pub fn analyze_all(&mut self, lines: &[String]) {
        let lexer = self.language.create_lexer();
        self.line_tokens = lines.iter().map(|line| lexer.lex_full(line)).collect();
        self.last_line_count = lines.len();
        self.version += 1;
    }

    /// 增量更新 - 根据编辑结果只重新分析受影响的行
    ///
    /// 编辑后，受影响的行包括：
    /// 1. 编辑起始行（内容可能改变）
    /// 2. 编辑结束行（内容可能改变）
    /// 3. 编辑范围内的所有行
    /// 4. 后续行（行号可能偏移）
    pub fn update_for_edit(&mut self, edit_result: &EditResult, lines: &[String]) {
        if lines.is_empty() {
            self.line_tokens.clear();
            self.last_line_count = 0;
            self.version += 1;
            return;
        }

        let start_line = edit_result.start_line;
        let end_line = edit_result.end_line.min(lines.len().saturating_sub(1));
        let line_delta = edit_result.line_delta;

        // 1. 调整行数（Vec 的 splice/resize 操作比 HashMap 重建快得多）
        if line_delta > 0 {
            // 行数增加：在 start_line 处插入空行
            let insert_count = line_delta as usize;
            let insert_pos = (start_line + 1).min(self.line_tokens.len());
            self.line_tokens.splice(
                insert_pos..insert_pos,
                (0..insert_count).map(|_| Vec::new()),
            );
        } else if line_delta < 0 {
            // 行数减少：移除被删除的行
            let remove_count = (-line_delta) as usize;
            let remove_start = (start_line + 1).min(self.line_tokens.len());
            let remove_end = (remove_start + remove_count).min(self.line_tokens.len());
            if remove_start < remove_end {
                self.line_tokens.drain(remove_start..remove_end);
            }
        }

        // 2. 确保 Vec 长度与 lines 一致
        self.line_tokens.resize_with(lines.len(), Vec::new);

        // 3. 重新分析受影响的行
        let dirty_start = start_line;
        let dirty_end = (end_line + 1).min(lines.len());

        let lexer = self.language.create_lexer();
        for line_idx in dirty_start..dirty_end {
            if let Some(line) = lines.get(line_idx) {
                self.line_tokens[line_idx] = lexer.lex_full(line);
            }
        }

        // 4. 如果行数增加，分析新增的行
        if line_delta > 0 {
            for line_idx in dirty_end..lines.len() {
                if self.line_tokens[line_idx].is_empty() {
                    if let Some(line) = lines.get(line_idx) {
                        self.line_tokens[line_idx] = lexer.lex_full(line);
                    }
                }
            }
        }

        self.last_line_count = lines.len();
        self.version += 1;
    }

    /// 获取指定行的token（从缓存）—— O(1) 直接索引
    pub fn get_line_tokens(&self, line_idx: usize) -> Option<&Vec<LexemeSpan>> {
        self.line_tokens.get(line_idx)
    }

    /// 获取所有行的token（用于渲染）—— 返回连续内存切片
    pub fn get_all_tokens(&self) -> &[Vec<LexemeSpan>] {
        &self.line_tokens
    }

    /// 获取缓存版本号
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 清除缓存（文件切换时使用）
    pub fn clear(&mut self) {
        self.line_tokens.clear();
        self.version = 0;
        self.last_line_count = 0;
    }

    /// 获取缓存命中率统计（调试用）
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.line_tokens.len(), self.last_line_count)
    }
}

/// 增量词法分析管理器
///
/// 管理多个文件的增量lexer，支持文件切换
pub struct IncrementalLexerManager {
    lexers: std::collections::HashMap<String, IncrementalLexer>,
    current_file: Option<String>,
}

impl IncrementalLexerManager {
    pub fn new() -> Self {
        Self {
            lexers: std::collections::HashMap::new(),
            current_file: None,
        }
    }

    /// 打开文件，获取或创建增量lexer
    pub fn open_file(&mut self, path: &str, language: Language) -> &mut IncrementalLexer {
        self.current_file = Some(path.to_string());
        self.lexers
            .entry(path.to_string())
            .or_insert_with(|| IncrementalLexer::new(language))
    }

    /// 获取当前文件的lexer
    pub fn current_lexer(&mut self) -> Option<&mut IncrementalLexer> {
        let current = self.current_file.as_ref()?;
        self.lexers.get_mut(current)
    }

    /// 关闭文件，释放lexer缓存
    pub fn close_file(&mut self, path: &str) {
        self.lexers.remove(path);
        if self.current_file.as_deref() == Some(path) {
            self.current_file = None;
        }
    }

    /// 切换当前文件
    pub fn switch_file(&mut self, path: &str) {
        self.current_file = Some(path.to_string());
    }

    /// 清除所有缓存
    pub fn clear_all(&mut self) {
        self.lexers.clear();
        self.current_file = None;
    }
}

impl Default for IncrementalLexerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_lexer_basic() {
        let mut lexer = IncrementalLexer::new(Language::Rust);
        let lines = vec![
            "fn main() {".to_string(),
            "    println!(\"hello\");".to_string(),
            "}".to_string(),
        ];

        lexer.analyze_all(&lines);
        assert_eq!(lexer.get_line_tokens(0).unwrap().len(), 7); // fn, ws, main, (, ), ws, {
        assert_eq!(lexer.get_line_tokens(1).unwrap().len(), 7); // indent, println!, !, (, "hello", ), ;
        assert_eq!(lexer.get_line_tokens(2).unwrap().len(), 1); // }
    }

    #[test]
    fn test_incremental_update_insert() {
        let mut lexer = IncrementalLexer::new(Language::Rust);
        let lines = vec!["fn main() {".to_string(), "}".to_string()];

        lexer.analyze_all(&lines);
        let v1 = lexer.version();

        // 模拟插入一行
        let new_lines = vec![
            "fn main() {".to_string(),
            "    let x = 1;".to_string(),
            "}".to_string(),
        ];
        let edit = EditResult::new(1, 1, 1);
        lexer.update_for_edit(&edit, &new_lines);

        assert!(lexer.version() > v1);
        assert_eq!(lexer.get_line_tokens(1).unwrap().len(), 9); // indent, let, ws, x, ws, =, ws, 1, ;
    }

    #[test]
    fn test_incremental_update_delete() {
        let mut lexer = IncrementalLexer::new(Language::Rust);
        let lines = vec![
            "fn main() {".to_string(),
            "    let x = 1;".to_string(),
            "    let y = 2;".to_string(),
            "}".to_string(),
        ];

        lexer.analyze_all(&lines);

        // 模拟删除中间行
        let new_lines = vec![
            "fn main() {".to_string(),
            "    let y = 2;".to_string(),
            "}".to_string(),
        ];
        let edit = EditResult::new(1, 2, -1);
        lexer.update_for_edit(&edit, &new_lines);

        assert_eq!(lexer.get_line_tokens(1).unwrap().len(), 9); // 原来的第2行变成第1行: indent, let, ws, y, ws, =, ws, 2, ;
    }
}
