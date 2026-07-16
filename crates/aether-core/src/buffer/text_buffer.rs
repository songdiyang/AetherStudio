/// 文本缓冲区核心 trait
/// 抽象所有文本编辑操作，使上层代码与具体数据结构（PieceTable/Rope）解耦
///
/// 设计原则：
/// - 所有操作基于字节偏移（byte offset），而非字符索引
/// - 行号从0开始
/// - 支持不可变快照（snapshot），用于后台线程安全访问
pub trait TextBuffer: Send + Sync {
    /// 在指定字节位置插入文本
    fn insert(&mut self, pos: usize, text: &str);

    /// 删除指定字节范围 [start, end)
    fn delete(&mut self, start: usize, end: usize);

    /// 获取指定字节范围的文本
    fn slice(&self, start: usize, end: usize) -> String;

    /// 获取全部文本
    fn full_text(&self) -> String;

    /// 获取总行数
    fn line_count(&self) -> usize;

    /// 获取总字节长度
    fn byte_len(&self) -> usize;

    /// 获取指定行的文本（不含换行符）
    fn line_text(&self, line_idx: usize) -> Option<String>;

    /// 获取指定行的字节范围 [start, end)
    fn line_byte_range(&self, line_idx: usize) -> Option<(usize, usize)>;

    /// 将行号+列号转换为字节偏移
    fn line_col_to_byte(&self, line: usize, col: usize) -> usize;

    /// 将字节偏移转换为行号+列号
    fn byte_to_line_col(&self, byte: usize) -> (usize, usize);

    /// 创建不可变快照（用于后台线程）
    /// 对于PieceTable，快照是轻量的piece列表副本
    /// 对于Rope，快照是Arc引用计数递增
    fn create_snapshot(&self) -> Box<dyn TextBufferSnapshot>;

    /// 保存当前状态（用于Undo）
    fn save_state(&self) -> BufferState;

    /// 恢复到之前保存的状态
    fn restore_state(&mut self, state: BufferState);
}

/// 不可变快照 trait
/// 允许后台线程安全读取缓冲区内容，无需锁
pub trait TextBufferSnapshot: Send + Sync {
    fn slice(&self, start: usize, end: usize) -> String;
    fn full_text(&self) -> String;
    fn line_count(&self) -> usize;
    fn line_text(&self, line_idx: usize) -> Option<String>;
    fn byte_len(&self) -> usize;
}

/// 缓冲区状态（用于Undo/Redo）
/// 轻量级的元数据快照，不包含实际文本内容
#[derive(Clone, Debug)]
pub struct BufferState {
    pub(crate) pieces_data: Vec<u8>, // 序列化的piece元数据
    #[allow(dead_code)]
    pub(crate) add_buffer_len: usize,
    pub(crate) line_count: usize,
    pub(crate) byte_len: usize,
}

impl BufferState {
    pub fn empty() -> Self {
        Self {
            pieces_data: Vec::new(),
            add_buffer_len: 0,
            line_count: 1,
            byte_len: 0,
        }
    }
}

/// 光标位置
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cursor {
    pub line: usize,
    pub col: usize, // 字节列
}

impl Cursor {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// 选择区域
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Selection {
    pub start: Cursor,
    pub end: Cursor,
}

impl Selection {
    pub fn new(start: Cursor, end: Cursor) -> Self {
        Self { start, end }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// 规范化：确保 start <= end
    pub fn normalized(&self) -> Self {
        if self.start.line < self.end.line
            || (self.start.line == self.end.line && self.start.col <= self.end.col)
        {
            *self
        } else {
            Self::new(self.end, self.start)
        }
    }
}

/// 编辑操作类型
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOp {
    Insert {
        pos: usize,
        text: String,
    },
    Delete {
        start: usize,
        end: usize,
    },
    Replace {
        start: usize,
        end: usize,
        text: String,
    },
}

/// 编辑操作结果，包含受影响的行范围
/// 用于行级缓存失效的精确计算
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditResult {
    /// 受影响的起始行号（包含）
    pub start_line: usize,
    /// 受影响的结束行号（包含）
    pub end_line: usize,
    /// 行数变化（正值表示增加，负值表示减少）
    pub line_delta: isize,
}

impl EditResult {
    pub fn new(start_line: usize, end_line: usize, line_delta: isize) -> Self {
        Self {
            start_line,
            end_line: end_line.max(start_line),
            line_delta,
        }
    }

    /// 合并两个编辑结果（用于批量操作）
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            start_line: self.start_line.min(other.start_line),
            end_line: self.end_line.max(other.end_line),
            line_delta: self.line_delta + other.line_delta,
        }
    }
}

/// 多光标编辑状态
/// 支持多个光标和选择区域
#[derive(Clone, Debug, Default)]
pub struct MultiCursorState {
    pub cursors: Vec<Cursor>,
    pub selections: Vec<Option<Selection>>,
    pub primary_cursor: usize, // 主光标索引
}

impl MultiCursorState {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::default()],
            selections: vec![None],
            primary_cursor: 0,
        }
    }

    /// H-05: 返回钳位后的主光标索引，避免外部设置非法值导致越界 panic。
    /// primary_cursor 字段保持 pub 以兼容现有代码，但所有内部访问都通过此方法。
    fn primary_idx(&self) -> usize {
        self.primary_cursor
            .min(self.cursors.len().saturating_sub(1))
    }

    pub fn primary_cursor(&self) -> &Cursor {
        let idx = self.primary_idx();
        &self.cursors[idx]
    }

    pub fn primary_cursor_mut(&mut self) -> &mut Cursor {
        let idx = self.primary_idx();
        &mut self.cursors[idx]
    }

    pub fn add_cursor(&mut self, cursor: Cursor) {
        self.cursors.push(cursor);
        self.selections.push(None);
    }

    pub fn clear_secondary_cursors(&mut self) {
        if self.cursors.len() > 1 {
            // H-05: 用钳位索引避免越界
            let idx = self.primary_idx();
            let primary = self.cursors[idx];
            let primary_sel = self.selections[idx];
            self.cursors = vec![primary];
            self.selections = vec![primary_sel];
            self.primary_cursor = 0;
        }
    }

    pub fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    /// 添加列选择模式的光标（矩形选区）
    pub fn add_column_cursors(
        &mut self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) {
        self.clear_secondary_cursors();
        let (first_line, first_col, last_line, last_col) = if start_line <= end_line {
            (start_line, start_col, end_line, end_col)
        } else {
            (end_line, end_col, start_line, start_col)
        };
        for line in first_line..=last_line {
            let col = if line == first_line {
                first_col
            } else {
                last_col.min(first_col)
            };
            self.add_cursor(Cursor::new(line, col));
        }
        self.primary_cursor = 0;
    }

    /// 检查是否处于列选择模式
    pub fn is_column_mode(&self) -> bool {
        self.cursors.len() > 1 && self.selections.iter().all(|s| s.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_state_empty() {
        let state = BufferState::empty();
        assert!(state.pieces_data.is_empty());
        assert_eq!(state.line_count, 1);
        assert_eq!(state.byte_len, 0);
    }

    #[test]
    fn test_selection_normalized() {
        let s = Selection::new(Cursor::new(1, 5), Cursor::new(2, 3));
        assert_eq!(s.normalized(), s);

        let reversed = Selection::new(Cursor::new(3, 10), Cursor::new(1, 2));
        let normalized = reversed.normalized();
        assert_eq!(normalized.start, Cursor::new(1, 2));
        assert_eq!(normalized.end, Cursor::new(3, 10));

        let same_line = Selection::new(Cursor::new(0, 5), Cursor::new(0, 2));
        let normalized = same_line.normalized();
        assert_eq!(normalized.start, Cursor::new(0, 2));
        assert_eq!(normalized.end, Cursor::new(0, 5));
    }

    #[test]
    fn test_selection_is_empty() {
        assert!(Selection::new(Cursor::new(0, 0), Cursor::new(0, 0)).is_empty());
        assert!(!Selection::new(Cursor::new(0, 0), Cursor::new(0, 1)).is_empty());
    }

    #[test]
    fn test_edit_result_merge() {
        let a = EditResult::new(2, 5, 1);
        let b = EditResult::new(4, 7, -1);
        let merged = a.merge(&b);
        assert_eq!(merged.start_line, 2);
        assert_eq!(merged.end_line, 7);
        assert_eq!(merged.line_delta, 0);
    }

    #[test]
    fn test_edit_result_end_line_clamped() {
        let r = EditResult::new(5, 3, 0);
        assert_eq!(r.end_line, 5);
    }

    #[test]
    fn test_multi_cursor_primary_idx_clamped() {
        let mut state = MultiCursorState::new();
        assert_eq!(state.cursor_count(), 1);
        state.add_cursor(Cursor::new(1, 0));
        state.primary_cursor = 100;
        assert_eq!(*state.primary_cursor(), Cursor::new(1, 0));
        *state.primary_cursor_mut() = Cursor::new(2, 5);
        assert_eq!(state.cursors[1], Cursor::new(2, 5));
    }

    #[test]
    fn test_clear_secondary_cursors() {
        let mut state = MultiCursorState::new();
        state.add_cursor(Cursor::new(1, 0));
        state.add_cursor(Cursor::new(2, 0));
        state.primary_cursor = 1;
        state.clear_secondary_cursors();
        assert_eq!(state.cursor_count(), 1);
        assert_eq!(*state.primary_cursor(), Cursor::new(1, 0));
        assert_eq!(state.primary_cursor, 0);
    }

    #[test]
    fn test_column_mode() {
        let mut state = MultiCursorState::default();
        assert!(!state.is_column_mode());
        state.add_column_cursors(0, 2, 3, 5);
        assert!(state.is_column_mode());
        assert_eq!(state.cursor_count(), 4);
        assert_eq!(state.cursors[0], Cursor::new(0, 2));
        assert_eq!(state.cursors[3], Cursor::new(3, 2));
    }
}
