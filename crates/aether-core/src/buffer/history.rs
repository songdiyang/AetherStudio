use std::collections::VecDeque;
use std::time::Instant;

use super::piece_table::Piece;

/// 基于Piece Table快照的高效Undo/Redo
/// 不是保存文本内容，而是保存piece表的元数据状态
pub struct History {
    /// 操作记录栈 — CORE-M02: 使用 VecDeque 替代 Vec，O(1) 淘汰而非 O(n) remove(0)
    undos: VecDeque<EditRecord>,
    redos: VecDeque<EditRecord>,
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
    /// 操作类型（影响合并策略）
    op_type: OpType,
    /// REQ-P0-02: 是否属于撤销组
    in_group: bool,
    /// REQ-P0-02: 是否是撤销组的第一条记录
    group_start: bool,
    /// 编辑时间戳
    timestamp: Instant,
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
    /// REQ-P0-02: 撤销组开始标记，下一个 record 为组的第一条记录
    GroupStart,
    /// REQ-P0-02: 撤销组进行中，组内记录不合并
    Grouping { first_time: Instant, first_pos: usize },
}

impl History {
    pub fn new() -> Self {
        Self {
            undos: VecDeque::new(),
            redos: VecDeque::new(),
            merge_state: MergeState::Idle,
            max_records: 10000,
        }
    }

    /// REQ-P0-02: 开始撤销组
    /// 在组内所有 record() 调用都不会合并，且组首记录被标记。
    /// undo() 会一次性撤销整个组。
    pub fn begin_group(&mut self) {
        self.merge_state = MergeState::GroupStart;
    }

    /// REQ-P0-02: 结束撤销组
    /// 恢复合并状态为 Idle，后续 record() 恢复正常合并行为。
    pub fn end_group(&mut self) {
        self.merge_state = MergeState::Idle;
    }

    /// 记录一次编辑操作
    /// 调用时机：在编辑完成后，传入编辑前的状态
    /// `edit_len`: 插入的字节长度（用于 Insert 合并判断；Delete 传 0）
    pub fn record(
        &mut self,
        before_pieces: Vec<Piece>,
        before_add_len: usize,
        cursor_before: CursorPosition,
        cursor_after: CursorPosition,
        op_type: OpType,
        edit_pos: usize,
        edit_len: usize,
    ) {
        let now = Instant::now();

        // REQ-P0-02: 在撤销组模式下，组内记录不合并
        let in_group_mode = matches!(
            self.merge_state,
            MergeState::GroupStart | MergeState::Grouping { .. }
        );
        let is_group_start = self.merge_state == MergeState::GroupStart;

        // 检查是否可以合并（组模式下不合并）
        let should_merge = if in_group_mode {
            false
        } else {
            match (self.merge_state, op_type) {
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
            }
        };

        if should_merge && !self.undos.is_empty() {
            // 更新合并组的最终状态
            if let Some(last) = self.undos.back_mut() {
                last.cursor_after = cursor_after;
            }
        } else {
            // 创建新记录
            let record = EditRecord {
                prev_pieces: before_pieces,
                prev_add_len: before_add_len,
                cursor_before,
                cursor_after,
                op_type,
                in_group: in_group_mode,
                group_start: is_group_start,
                timestamp: now,
            };
            self.undos.push_back(record);
            self.redos.clear(); // 新操作清空redo栈

            // 限制记录数量 — CORE-M02: O(1) pop_front 替代 O(n) remove(0)
            while self.undos.len() > self.max_records {
                self.undos.pop_front();
            }
        }

        // 更新合并状态
        if in_group_mode {
            // REQ-P0-02: 组模式下保持 Grouping 状态
            self.merge_state = MergeState::Grouping {
                first_time: now,
                first_pos: edit_pos,
            };
        } else {
            self.merge_state = match op_type {
                OpType::Insert => MergeState::Inserting {
                    last_time: now,
                    // H-15: 使用实际字节长度而非 +1，正确处理多字节 UTF-8 字符的连续合并
                    last_pos: edit_pos + edit_len,
                },
                OpType::Delete => MergeState::Deleting {
                    last_time: now,
                    last_pos: edit_pos,
                },
                OpType::Replace => MergeState::Idle,
            };
        }
    }

    /// 撤销一次操作
    /// 返回：编辑前的pieces状态（用于恢复）
    /// REQ-P0-02: 支持撤销组——如果最后一条记录属于组但不是组首，
    /// 则连续弹出直到组首，返回组首的 prev_pieces。
    pub fn undo(
        &mut self,
        current_pieces: Vec<Piece>,
        current_add_len: usize,
        current_cursor: CursorPosition,
    ) -> Option<(Vec<Piece>, usize, CursorPosition)> {
        let record = self.undos.pop_back()?;

        // REQ-P0-02: 如果记录是组成员但非组首，继续弹出直到组首
        if record.in_group && !record.group_start {
            // 收集弹出的组成员记录（不含组首）
            let mut group_members = vec![record];
            let mut group_start_record = None;

            while let Some(r) = self.undos.pop_back() {
                if r.in_group && r.group_start {
                    group_start_record = Some(r);
                    break;
                }
                if !r.in_group {
                    // 遇到非组记录，推回并停止（防御性编程）
                    self.undos.push_back(r);
                    break;
                }
                group_members.push(r);
            }

            if let Some(start) = group_start_record {
                // 将当前状态保存到 redo 栈作为单条汇总记录
                self.redos.push_back(EditRecord {
                    prev_pieces: current_pieces,
                    prev_add_len: current_add_len,
                    cursor_before: start.cursor_before,
                    cursor_after: current_cursor,
                    timestamp: start.timestamp,
                    op_type: start.op_type,
                    in_group: true,
                    group_start: true,
                });

                self.merge_state = MergeState::Idle;
                let _ = group_members; // 丢弃中间记录
                return Some((start.prev_pieces, start.prev_add_len, start.cursor_before));
            }

            // 未找到组首（不应发生），回退为单条撤销——使用第一个组成员
            let first = group_members.into_iter().next().unwrap();
            let cursor = first.cursor_before;
            self.redos.push_back(EditRecord {
                prev_pieces: current_pieces,
                prev_add_len: current_add_len,
                cursor_before: cursor,
                cursor_after: current_cursor,
                timestamp: first.timestamp,
                op_type: first.op_type,
                in_group: first.in_group,
                group_start: first.group_start,
            });
            self.merge_state = MergeState::Idle;
            return Some((first.prev_pieces, first.prev_add_len, cursor));
        }

        let cursor = record.cursor_before;

        // 保存当前状态到redo栈
        self.redos.push_back(EditRecord {
            prev_pieces: current_pieces,
            prev_add_len: current_add_len,
            cursor_before: cursor,
            cursor_after: current_cursor,
            timestamp: record.timestamp,
            op_type: record.op_type,
            in_group: record.in_group,
            group_start: record.group_start,
        });

        self.merge_state = MergeState::Idle;
        Some((record.prev_pieces, record.prev_add_len, cursor))
    }

    /// 重做一次操作
    /// REQ-P0-02: 如果 redo 栈顶是组首记录，直接返回其 prev_pieces
    /// （组撤销时保存的是单条汇总记录，所以 redo 也是单条）
    pub fn redo(
        &mut self,
        current_pieces: Vec<Piece>,
        current_add_len: usize,
        current_cursor: CursorPosition,
    ) -> Option<(Vec<Piece>, usize, CursorPosition)> {
        let record = self.redos.pop_back()?;
        let cursor = record.cursor_after;

        // 保存当前状态到undo栈
        self.undos.push_back(EditRecord {
            prev_pieces: current_pieces,
            prev_add_len: current_add_len,
            cursor_before: current_cursor,
            cursor_after: cursor,
            timestamp: record.timestamp,
            op_type: record.op_type,
            in_group: record.in_group,
            group_start: record.group_start,
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
            1,
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
            1,
        );
        history.record(
            pieces.clone(),
            1,
            CursorPosition::new(0, 1),
            CursorPosition::new(0, 2),
            OpType::Insert,
            1,
            1,
        );

        assert_eq!(history.undos.len(), 1);
    }

    #[test]
    fn test_redo_after_undo() {
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
            5,
        );
        let _ = history.undo(pieces2.clone(), 10, CursorPosition::new(0, 10));
        let redo_result = history.redo(pieces1.clone(), 5, CursorPosition::new(0, 0));
        assert!(redo_result.is_some());
        let (_, _, cursor) = redo_result.unwrap();
        // redo 返回的是 redo 记录保存的 cursor_after，即 undo 时的 current_cursor
        assert_eq!(cursor, CursorPosition::new(0, 10));
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_new_record_clears_redo() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];

        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 5),
            OpType::Insert,
            0,
            5,
        );
        let _ = history.undo(pieces.clone(), 5, CursorPosition::new(0, 5));
        assert!(history.can_redo());

        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 1),
            OpType::Insert,
            0,
            1,
        );
        assert!(!history.can_redo());
    }

    #[test]
    fn test_history_clear() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 5),
            OpType::Insert,
            0,
            5,
        );
        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_undos_limit() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 1,
            line_breaks: 0,
        }];
        for i in 0..10010 {
            history.record(
                pieces.clone(),
                1,
                CursorPosition::new(0, i),
                CursorPosition::new(0, i + 1),
                OpType::Replace,
                0,
                1,
            );
        }
        assert_eq!(history.undos.len(), 10000);
    }

    #[test]
    fn test_merge_deletes() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 5),
            CursorPosition::new(0, 4),
            OpType::Delete,
            4,
            0,
        );
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 4),
            CursorPosition::new(0, 3),
            OpType::Delete,
            4,
            0,
        );
        assert_eq!(history.undos.len(), 1);
    }

    #[test]
    fn test_replace_not_merged() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 1),
            OpType::Replace,
            0,
            1,
        );
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 1),
            CursorPosition::new(0, 2),
            OpType::Replace,
            1,
            1,
        );
        assert_eq!(history.undos.len(), 2);
    }

    #[test]
    fn test_group_undo() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];

        // 开始组
        history.begin_group();

        // 记录3次替换
        for i in 0..3 {
            history.record(
                pieces.clone(),
                5,
                CursorPosition::new(0, i),
                CursorPosition::new(0, i + 1),
                OpType::Replace,
                i,
                1,
            );
        }

        // 结束组
        history.end_group();

        // 应该有3条记录
        assert_eq!(history.undos.len(), 3);

        // 一次 undo 应该撤销整个组
        let result = history.undo(pieces.clone(), 5, CursorPosition::new(0, 3));
        assert!(result.is_some());
        let (_, _, cursor) = result.unwrap();
        // 光标应该回到组首记录的 cursor_before
        assert_eq!(cursor, CursorPosition::new(0, 0));

        // undo 后 undo 栈应该清空
        assert!(!history.can_undo());

        // redo 应该恢复整个组
        let redo_result = history.redo(pieces.clone(), 5, CursorPosition::new(0, 0));
        assert!(redo_result.is_some());
        let (_, _, redo_cursor) = redo_result.unwrap();
        // 光标应该回到组操作后的位置
        assert_eq!(redo_cursor, CursorPosition::new(0, 3));
    }

    #[test]
    fn test_group_records_not_merged() {
        let mut history = History::new();
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];

        history.begin_group();

        // 组内连续插入（相同位置，快速连续）应该不合并
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 0),
            CursorPosition::new(0, 1),
            OpType::Insert,
            0,
            1,
        );
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 1),
            CursorPosition::new(0, 2),
            OpType::Insert,
            1,
            1,
        );
        history.record(
            pieces.clone(),
            5,
            CursorPosition::new(0, 2),
            CursorPosition::new(0, 3),
            OpType::Insert,
            2,
            1,
        );

        history.end_group();

        // 3条记录都不合并
        assert_eq!(history.undos.len(), 3);
    }
}
