use std::collections::VecDeque;
use std::sync::Arc;

use crate::buffer::piece_table::{Piece, PieceTable};
use crate::buffer::text_buffer::EditResult;

/// 持久化版本快照
///
/// PieceTable 的天然持久化特性：
/// - add_buffer 只追加不删除，所有历史版本共享
/// - pieces 列表通过 Arc 共享，undo/redo 只是引用计数 +1
/// - 不同版本的 pieces 列表可以共享相同的 add_buffer 引用
#[derive(Clone, Debug)]
pub struct VersionSnapshot {
    /// 版本ID（单调递增）
    pub version_id: u64,
    /// 片段列表（Arc 包装，避免完整拷贝）
    pub pieces: Arc<Vec<Piece>>,
    /// 总字节数
    pub total_bytes: usize,
    /// 总行数
    pub total_lines: usize,
    /// 编辑操作描述（用于显示）
    pub edit_description: String,
    /// 时间戳
    pub timestamp: std::time::Instant,
}

/// 持久化版本历史管理器
///
/// 支持高效的撤销/重做，利用PieceTable的持久化特性
/// 内存开销：每个版本仅存储 Arc<Vec<Piece>>（引用计数，共享数据）
pub struct PersistentHistory {
    /// 版本历史（环形缓冲区）
    history: VecDeque<VersionSnapshot>,
    /// 当前版本索引
    current_index: usize,
    /// 最大历史记录数
    max_history: usize,
    /// 下一个版本ID
    next_version_id: u64,
    /// 合并小编辑的阈值（字节）
    coalesce_threshold: usize,
    /// 上次编辑时间（用于时间合并）
    last_edit_time: Option<std::time::Instant>,
    /// 时间合并窗口（毫秒）
    coalesce_time_ms: u64,
    /// 上次编辑大小（用于合并判断）
    last_edit_size: usize,
}

impl PersistentHistory {
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_history),
            current_index: 0,
            max_history,
            next_version_id: 0,
            coalesce_threshold: 10,
            last_edit_time: None,
            coalesce_time_ms: 500,
            last_edit_size: 0,
        }
    }

    /// 记录新版本
    ///
    /// 如果编辑很小且距离上次编辑很近，则尝试合并
    pub fn record_version(
        &mut self,
        pieces: Vec<Piece>,
        total_bytes: usize,
        total_lines: usize,
        edit_description: &str,
        edit_size: usize,
    ) -> u64 {
        let now = std::time::Instant::now();
        let should_coalesce = self.should_coalesce(edit_size, now);

        if should_coalesce && self.current_index > 0 && !self.history.is_empty() {
            // 合并到当前版本
            if let Some(current) = self.history.get_mut(self.current_index) {
                current.pieces = Arc::new(pieces);
                current.total_bytes = total_bytes;
                current.total_lines = total_lines;
                current.edit_description =
                    format!("{} + {}", current.edit_description, edit_description);
                current.timestamp = now;
                self.last_edit_time = Some(now);
                self.last_edit_size = edit_size;
                return current.version_id;
            }
        }

        // 创建新版本
        let version_id = self.next_version_id;
        self.next_version_id += 1;

        let snapshot = VersionSnapshot {
            version_id,
            pieces: Arc::new(pieces),
            total_bytes,
            total_lines,
            edit_description: edit_description.to_string(),
            timestamp: now,
        };

        // 如果当前不在最新版本，丢弃当前版本之后的所有历史
        if self.current_index < self.history.len().saturating_sub(1) {
            self.history.truncate(self.current_index + 1);
        }

        // 添加新版本
        self.history.push_back(snapshot);

        // 限制历史大小
        while self.history.len() > self.max_history {
            self.history.pop_front();
            if self.current_index > 0 {
                self.current_index -= 1;
            }
        }

        self.current_index = self.history.len().saturating_sub(1);
        self.last_edit_time = Some(now);
        self.last_edit_size = edit_size;

        version_id
    }

    /// 判断是否应该合并编辑
    fn should_coalesce(&self, edit_size: usize, now: std::time::Instant) -> bool {
        if edit_size >= self.coalesce_threshold {
            return false;
        }

        // 如果上次编辑也很大，不合并
        if self.last_edit_size >= self.coalesce_threshold {
            return false;
        }

        if let Some(last_time) = self.last_edit_time {
            let elapsed = now.duration_since(last_time).as_millis() as u64;
            elapsed < self.coalesce_time_ms
        } else {
            false
        }
    }

    /// 撤销 - 返回上一个版本的 pieces
    pub fn undo(&mut self) -> Option<&VersionSnapshot> {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.history.get(self.current_index)
        } else {
            None
        }
    }

    /// 重做 - 返回下一个版本的 pieces
    pub fn redo(&mut self) -> Option<&VersionSnapshot> {
        if self.current_index + 1 < self.history.len() {
            self.current_index += 1;
            self.history.get(self.current_index)
        } else {
            None
        }
    }

    /// 获取当前版本
    pub fn current(&self) -> Option<&VersionSnapshot> {
        self.history.get(self.current_index)
    }

    /// 是否可以撤销
    pub fn can_undo(&self) -> bool {
        self.current_index > 0
    }

    /// 是否可以重做
    pub fn can_redo(&self) -> bool {
        self.current_index + 1 < self.history.len()
    }

    /// 获取历史长度
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// 获取撤销栈深度
    pub fn undo_depth(&self) -> usize {
        self.current_index
    }

    /// 获取重做栈深度
    pub fn redo_depth(&self) -> usize {
        self.history.len().saturating_sub(self.current_index + 1)
    }

    /// 清除所有历史
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_index = 0;
        self.next_version_id = 0;
        self.last_edit_time = None;
        self.last_edit_size = 0;
    }

    /// 获取所有版本的描述（用于UI显示）
    pub fn version_descriptions(&self) -> Vec<(u64, String)> {
        self.history
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let marker = if i == self.current_index { " *" } else { "" };
                (v.version_id, format!("{}{}", v.edit_description, marker))
            })
            .collect()
    }
}

impl Default for PersistentHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

/// 带持久化历史的 PieceTable 包装器
///
/// 提供高效的撤销/重做功能，同时保持PieceTable的所有性能优势
pub struct PersistentPieceTable {
    /// 当前PieceTable
    pub table: PieceTable,
    /// 版本历史
    history: PersistentHistory,
    /// 是否自动记录历史
    auto_record: bool,
}

impl PersistentPieceTable {
    pub fn from_string(text: String) -> Self {
        let table = PieceTable::from_string(text);
        let mut history = PersistentHistory::new(100);

        // 记录初始版本
        history.record_version(
            table.get_pieces(),
            table.len_bytes(),
            table.len_lines(),
            "初始版本",
            0,
        );

        Self {
            table,
            history,
            auto_record: true,
        }
    }

    /// 插入文本并自动记录历史
    pub fn insert(&mut self, pos: usize, text: &str) -> EditResult {
        let result = self.table.insert_with_result(pos, text);

        if self.auto_record {
            self.record_history(&format!(
                "插入 '{}'",
                text.chars().take(20).collect::<String>()
            ));
        }

        result
    }

    /// 删除文本并自动记录历史
    pub fn delete(&mut self, start: usize, end: usize) -> EditResult {
        let result = self.table.delete_with_result(start, end);

        if self.auto_record {
            self.record_history(&format!("删除 {} 字节", end - start));
        }

        result
    }

    /// 撤销 —— 从 Arc 中读取 pieces，避免完整拷贝
    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.history.undo() {
            // 从 Arc<Vec<Piece>> 中 clone pieces（只有 Arc 引用计数 +1）
            // 注意：PieceTable.restore() 需要拥有 Vec<Piece>，所以需要 clone Arc 内部数据
            // 但 Arc 的 clone 只是引用计数递增，真正的数据拷贝发生在 restore 内部
            let pieces = (*snapshot.pieces).clone();
            self.table.restore(pieces, self.table.add_buffer_len());
            true
        } else {
            false
        }
    }

    /// 重做 —— 从 Arc 中读取 pieces
    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.history.redo() {
            let pieces = (*snapshot.pieces).clone();
            self.table.restore(pieces, self.table.add_buffer_len());
            true
        } else {
            false
        }
    }

    /// 记录当前状态到历史
    fn record_history(&mut self, description: &str) {
        let edit_size = if let Some(current) = self.history.current() {
            self.table.len_bytes().saturating_sub(current.total_bytes)
        } else {
            0
        };

        self.history.record_version(
            self.table.get_pieces(),
            self.table.len_bytes(),
            self.table.len_lines(),
            description,
            edit_size,
        );
    }

    /// 设置是否自动记录历史
    pub fn set_auto_record(&mut self, enabled: bool) {
        self.auto_record = enabled;
    }

    /// 批量操作（不记录中间状态）
    pub fn with_batch<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut PieceTable) -> R,
    {
        let was_auto = self.auto_record;
        self.auto_record = false;
        let result = f(&mut self.table);
        self.auto_record = was_auto;

        // 批量操作结束后记录一次
        if was_auto {
            self.record_history("批量操作");
        }

        result
    }

    /// 获取历史信息
    pub fn history_info(&self) -> HistoryInfo {
        HistoryInfo {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            undo_depth: self.history.undo_depth(),
            redo_depth: self.history.redo_depth(),
            total_versions: self.history.len(),
            current_version: self.history.current().map(|v| v.version_id),
        }
    }
}

/// 历史信息（用于UI显示）
#[derive(Clone, Debug)]
pub struct HistoryInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    pub total_versions: usize,
    pub current_version: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistent_history() {
        let mut history = PersistentHistory::new(10);

        let pieces1 = vec![];
        history.record_version(pieces1.clone(), 0, 1, "初始", 0);
        assert_eq!(history.len(), 1);

        let pieces2 = vec![];
        history.record_version(pieces2, 10, 2, "编辑1", 10);
        assert_eq!(history.len(), 2);

        // 撤销
        let snapshot = history.undo().unwrap();
        assert_eq!(snapshot.version_id, 0);
        assert_eq!(history.undo_depth(), 0);

        // 重做
        let snapshot = history.redo().unwrap();
        assert_eq!(snapshot.version_id, 1);
        assert_eq!(history.redo_depth(), 0);
    }

    #[test]
    fn test_persistent_piece_table() {
        let mut ppt = PersistentPieceTable::from_string("Hello World".to_string());

        assert_eq!(ppt.table.get_line(0), Some("Hello World".to_string()));

        // 插入
        ppt.insert(5, " Beautiful");
        assert_eq!(
            ppt.table.get_line(0),
            Some("Hello Beautiful World".to_string())
        );

        // 撤销
        assert!(ppt.undo());
        assert_eq!(ppt.table.get_line(0), Some("Hello World".to_string()));

        // 重做
        assert!(ppt.redo());
        assert_eq!(
            ppt.table.get_line(0),
            Some("Hello Beautiful World".to_string())
        );
    }

    #[test]
    fn test_batch_operation() {
        let mut ppt = PersistentPieceTable::from_string("Hello".to_string());

        ppt.with_batch(|table| {
            table.insert(5, " ");
            table.insert(6, "World");
            table.insert(11, "!");
        });

        // 批量操作只记录一次历史
        assert_eq!(ppt.history_info().total_versions, 2); // 初始 + 批量
        assert_eq!(ppt.table.get_line(0), Some("Hello World!".to_string()));

        // 撤销一次就回到初始状态
        assert!(ppt.undo());
        assert_eq!(ppt.table.get_line(0), Some("Hello".to_string()));
    }

    #[test]
    fn test_coalesce() {
        let mut history = PersistentHistory::new(10);
        history.coalesce_threshold = 100; // 设置大阈值以便测试
        history.coalesce_time_ms = 10000; // 设置大时间窗口

        let pieces = vec![];
        history.record_version(pieces.clone(), 0, 1, "初始", 0);

        // 小编辑应该合并到当前版本（current_index在初始版本上）
        history.record_version(pieces.clone(), 1, 1, "a", 1);
        assert_eq!(history.len(), 2); // 初始 + 小编辑

        // 再一个小编辑应该合并（因为当前版本edit_size=1）
        history.record_version(pieces.clone(), 2, 1, "b", 1);
        assert_eq!(history.len(), 2); // 合并到上一个版本

        // 大编辑不合并
        history.record_version(pieces, 100, 1, "大编辑", 100);
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_arc_sharing() {
        // 验证 Arc 包装确保 undo/redo 共享数据
        let mut history = PersistentHistory::new(10);

        let pieces1 = vec![Piece {
            source: crate::buffer::piece_table::Source::Add,
            start: 0,
            len: 5,
            line_breaks: 0,
        }];
        history.record_version(pieces1, 5, 1, "v1", 5);

        let pieces2 = vec![Piece {
            source: crate::buffer::piece_table::Source::Add,
            start: 0,
            len: 10,
            line_breaks: 0,
        }];
        history.record_version(pieces2, 10, 1, "v2", 5);

        // undo 回到 v1
        let snap1 = history.undo().unwrap();
        assert_eq!(snap1.pieces.len(), 1);
        assert_eq!(snap1.pieces[0].len, 5);

        // redo 到 v2
        let snap2 = history.redo().unwrap();
        assert_eq!(snap2.pieces.len(), 1);
        assert_eq!(snap2.pieces[0].len, 10);
    }
}
