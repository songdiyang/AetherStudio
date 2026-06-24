use std::time::Instant;

use super::piece_table::Piece;

/// 基于Piece Table快照的高效Undo/Redo
/// 不是保存文本内容，而是保存piece表的元数据状态
pub struct History {
    /// 操作记录栈
    undos: Vec<EditRecord>,
    redos: Vec<EditRecord>,
    /// 合并窗口（连续输入合并为一个undo组）
    merge_state: MergeState,
    /// 最大记录数（默认10000）
    max_records: usize,
}

/// 单次编辑记录（极轻量）
#[derive(Clone, Debug)]
pub struct EditRecord {
    /// 编辑前的pieces状态（完整副本）
    prev_pieces: Vec<Piece>,
    /// 编辑前的add_buffer长度
    prev_add_len: usize,
    /// 编辑位置
    pub cursor_before: CursorPosition,
    pub cursor_after: CursorPosition,
    /// 时间戳（用于合并判断）
    timestamp: Instant,
    /// 操作类型（影响合并策略）
    op_type: OpType,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
}

impl CursorPosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpType {
    Insert,
    Delete,
    Replace,
}

/// 合并状态
#[derive(Clone, Copy, Debug, PartialEq)]
enum MergeState {
    Idle,
    Inserting { last_time: Instant, last_pos: usize },
    Deleting { last_time: Instant, last_pos: usize },
}

impl History {
    pub fn new() -> Self {
        Self {
            undos: Vec::new(),
            redos: Vec::new(),
            merge_state: MergeState::Idle,
            max_records: 10000,
        }
    }

    /// 记录一次编辑操作
    /// 调用时机：在编辑完成后，传入编辑前的状态
    pub fn record(
        &mut self,
        before_pieces: Vec<Piece>,
        before_add_len: usize,
        cursor_before: CursorPosition,
        cursor_after: CursorPosition,
        op_type: OpType,
        edit_pos: usize,
    ) {
        let now = Instant::now();

        // 检查是否可以合并
        let should_merge = match (self.merge_state, op_type) {
            (
                MergeState::Inserting {
                    last_time,
                    last_pos,
                },
                OpType::Insert,
            ) => {
                let elapsed = now.duration_since(last_time).as_millis();
                elapsed < 500 && edit_pos == last_pos
            }
            (
                MergeState::Deleting {
                    last_time,
                    last_pos,
                },
                OpType::Delete,
            ) => {
                let elapsed = now.duration_since(last_time).as_millis();
                elapsed < 500 && edit_pos == last_pos
            }
            _ => false,
        };

        if should_merge && !self.undos.is_empty() {
            // 更新合并组的最终状态
            if let Some(last) = self.undos.last_mut() {
                last.cursor_after = cursor_after;
            }
        } else {
            // 创建新记录
            let record = EditRecord {
                prev_pieces: before_pieces,
                prev_add_len: before_add_len,
                cursor_before,
                cursor_after,
                timestamp: now,
                op_type,
            };
            self.undos.push(record);
            self.redos.clear(); // 新操作清空redo栈

            // 限制记录数量
            if self.undos.len() > self.max_records {
                self.undos.remove(0);
            }
        }

        // 更新合并状态
        self.merge_state = match op_type {
            OpType::Insert => MergeState::Inserting {
                last_time: now,
                last_pos: edit_pos + 1,
            },
            OpType::Delete => MergeState::Deleting {
                last_time: now,
                last_pos: edit_pos,
            },
            OpType::Replace => MergeState::Idle,
        };
    }

    /// 撤销一次操作
    /// 返回：编辑前的pieces状态（用于恢复）
    pub fn undo(
        &mut self,
        current_pieces: Vec<Piece>,
        current_add_len: usize,
        current_cursor: CursorPosition,
    ) -> Option<(Vec<Piece>, usize, CursorPosition)> {
        let record = self.undos.pop()?;
        let cursor = record.cursor_before.clone();

        // 保存当前状态到redo栈
        self.redos.push(EditRecord {
            prev_pieces: current_pieces,
            prev_add_len: current_add_len,
            cursor_before: cursor.clone(),
            cursor_after: current_cursor,
            timestamp: record.timestamp,
            op_type: record.op_type,
        });

        self.merge_state = MergeState::Idle;
        Some((record.prev_pieces, record.prev_add_len, cursor))
    }

    /// 重做一次操作
    pub fn redo(
        &mut self,
        current_pieces: Vec<Piece>,
        current_add_len: usize,
        current_cursor: CursorPosition,
    ) -> Option<(Vec<Piece>, usize, CursorPosition)> {
        let record = self.redos.pop()?;
        let cursor = record.cursor_after.clone();

        // 保存当前状态到undo栈
        self.undos.push(EditRecord {
            prev_pieces: current_pieces,
            prev_add_len: current_add_len,
            cursor_before: current_cursor,
            cursor_after: cursor.clone(),
            timestamp: record.timestamp,
            op_type: record.op_type,
        });

        self.merge_state = MergeState::Idle;
        Some((record.prev_pieces, record.prev_add_len, cursor))
    }

    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }

    pub fn clear(&mut self) {
        self.undos.clear();
        self.redos.clear();
        self.merge_state = MergeState::Idle;
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::piece_table::Source;

    #[test]
    fn test_undo_redo() {
        let mut history = History::new();
        let pieces1 = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];
        let pieces2 = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 10,
            line_breaks: 0,
        }];

        history.record(
            pieces1.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 5),
            OpType::Insert,
            0,
        );

        let result = history.undo(pieces2.clone(), 10, CursorPosition::new(0, 10));
        assert!(result.is_some());
        let (restored_pieces, restored_len, cursor) = result.unwrap();
        assert_eq!(restored_pieces.len(), pieces1.len());
        assert_eq!(restored_len, 5);
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);

        assert!(history.can_redo());
    }

    #[test]
    fn test_merge_inserts() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 0,
            line_breaks: 0,
        }];

        // 快速连续插入应该合并
        history.record(
            pieces.clone(),
            0,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 1),
            OpType::Insert,
            0,
        );
        history.record(
            pieces.clone(),
            1,
            CursorPosition::new(0, 1),
            CursorPosition::new(0, 2),
            OpType::Insert,
            1,
        );

        assert_eq!(history.undos.len(), 1);
    }
}
