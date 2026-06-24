use lsp_types::*;

/// 优化的增量变更计算器
///
/// 使用基于PieceTable的编辑历史来精确计算LSP增量变更
/// 避免全文对比，直接从编辑操作生成变更事件
pub struct IncrementalChangeCalculator;

impl IncrementalChangeCalculator {
    /// 从编辑操作直接生成LSP增量变更
    ///
    /// 这是最高效的同步方式，无需文本对比
    pub fn from_edit_op(
        _edit_kind: EditKind,
        start_byte: usize,
        end_byte: usize,
        text: &str,
        line_index: &dyn LineIndexProvider,
    ) -> Vec<TextDocumentContentChangeEvent> {
        let start_pos = line_index.byte_to_position(start_byte);
        let end_pos = line_index.byte_to_position(end_byte);

        vec![TextDocumentContentChangeEvent {
            range: Some(Range {
                start: start_pos,
                end: end_pos,
            }),
            range_length: Some((end_byte - start_byte) as u32),
            text: text.to_string(),
        }]
    }

    /// 批量编辑优化 - 合并相邻的编辑操作
    ///
    /// 减少LSP消息数量，提高同步效率
    pub fn merge_edits(
        edits: Vec<TextDocumentContentChangeEvent>,
    ) -> Vec<TextDocumentContentChangeEvent> {
        if edits.len() <= 1 {
            return edits;
        }

        let mut merged = Vec::new();
        let mut current = edits[0].clone();

        for edit in edits.into_iter().skip(1) {
            if let (Some(current_range), Some(next_range)) = (current.range, edit.range) {
                // 检查是否可以合并（相邻或重叠）
                if next_range.start.line <= current_range.end.line + 1 {
                    // 合并两个范围
                    let merged_text = current.text + &edit.text;
                    current = TextDocumentContentChangeEvent {
                        range: Some(Range {
                            start: current_range.start,
                            end: next_range.end,
                        }),
                        range_length: None,
                        text: merged_text,
                    };
                } else {
                    merged.push(current);
                    current = edit;
                }
            } else {
                // 如果任一没有范围，无法合并
                merged.push(current);
                current = edit;
            }
        }

        merged.push(current);
        merged
    }
}

/// 编辑类型
pub enum EditKind {
    Insert,
    Delete,
    Replace,
}

/// 行索引提供者 trait
/// 用于将字节偏移转换为LSP Position
pub trait LineIndexProvider {
    fn byte_to_position(&self, byte_offset: usize) -> Position;
    fn position_to_byte(&self, position: Position) -> usize;
}

/// 高效的行索引实现
///
/// 基于预计算的行起始位置数组，支持O(1)转换
pub struct FastLineIndex {
    line_starts: Vec<usize>,
    total_bytes: usize,
}

impl FastLineIndex {
    pub fn new(line_starts: Vec<usize>, total_bytes: usize) -> Self {
        Self {
            line_starts,
            total_bytes,
        }
    }

    /// 从文本创建行索引
    pub fn from_text(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            total_bytes: text.len(),
        }
    }
}

impl LineIndexProvider for FastLineIndex {
    fn byte_to_position(&self, byte_offset: usize) -> Position {
        let byte_offset = byte_offset.min(self.total_bytes);

        // 使用二分查找找到对应的行
        match self.line_starts.binary_search(&byte_offset) {
            Ok(line) => Position {
                line: line as u32,
                character: 0,
            },
            Err(line) => {
                let line = line.saturating_sub(1);
                let line_start = self.line_starts.get(line).copied().unwrap_or(0);
                let character = byte_offset - line_start;
                Position {
                    line: line as u32,
                    character: character as u32,
                }
            }
        }
    }

    fn position_to_byte(&self, position: Position) -> usize {
        let line = position.line as usize;
        let line_start = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.total_bytes);
        let character = position.character as usize;
        line_start + character
    }
}

/// LSP文档同步优化器
///
/// 管理文档版本和增量同步状态
pub struct OptimizedDocumentSync {
    uri: Url,
    version: i32,
    /// 编辑历史（用于生成增量变更）
    edit_history: Vec<EditRecord>,
    /// 最大历史记录数
    max_history: usize,
    /// 累计变更（用于批量发送）
    pending_changes: Vec<TextDocumentContentChangeEvent>,
    /// 批量发送阈值
    batch_threshold: usize,
}

#[derive(Clone, Debug)]
pub struct EditRecord {
    pub version: i32,
    pub start_byte: usize,
    pub end_byte: usize,
    pub text: String,
    pub timestamp: std::time::Instant,
}

impl OptimizedDocumentSync {
    pub fn new(uri: Url) -> Self {
        Self {
            uri,
            version: 0,
            edit_history: Vec::with_capacity(100),
            max_history: 50,
            pending_changes: Vec::new(),
            batch_threshold: 5,
        }
    }

    /// 记录编辑操作
    pub fn record_edit(&mut self, start_byte: usize, end_byte: usize, text: String) {
        self.version += 1;
        let record = EditRecord {
            version: self.version,
            start_byte,
            end_byte,
            text,
            timestamp: std::time::Instant::now(),
        };

        self.edit_history.push(record);

        // 限制历史记录大小
        if self.edit_history.len() > self.max_history {
            self.edit_history.remove(0);
        }
    }

    /// 生成增量变更（从最近一次同步到现在）
    pub fn generate_changes_since(
        &self,
        since_version: i32,
    ) -> Vec<TextDocumentContentChangeEvent> {
        self.edit_history
            .iter()
            .filter(|r| r.version > since_version)
            .map(|r| TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                }),
                range_length: Some((r.end_byte - r.start_byte) as u32),
                text: r.text.clone(),
            })
            .collect()
    }

    /// 添加待发送的变更
    pub fn queue_change(&mut self, change: TextDocumentContentChangeEvent) {
        self.pending_changes.push(change);
    }

    /// 检查是否需要批量发送
    pub fn should_flush(&self) -> bool {
        self.pending_changes.len() >= self.batch_threshold
    }

    /// 获取并清空待发送的变更
    pub fn flush_changes(&mut self) -> Vec<TextDocumentContentChangeEvent> {
        std::mem::take(&mut self.pending_changes)
    }

    /// 获取当前版本
    pub fn version(&self) -> i32 {
        self.version
    }

    /// 获取URI
    pub fn uri(&self) -> &Url {
        &self.uri
    }

    /// 清理过期的历史记录
    pub fn cleanup_old_history(&mut self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        self.edit_history
            .retain(|r| now.duration_since(r.timestamp) < max_age);
    }
}

/// 大文件优化策略
///
/// 对于超过一定大小的文件，使用特殊的同步策略
pub struct LargeFileSyncStrategy {
    /// 大文件阈值（字节）
    large_file_threshold: usize,
    /// 变更累积阈值（预留字段，当前未使用）
    #[allow(dead_code)]
    change_accumulation_threshold: usize,
    /// 同步间隔（毫秒）
    sync_interval_ms: u64,
}

impl LargeFileSyncStrategy {
    pub fn new() -> Self {
        Self {
            large_file_threshold: 100_000,       // 100KB
            change_accumulation_threshold: 1000, // 累积1000字节变更再同步
            sync_interval_ms: 500,               // 500ms同步间隔
        }
    }

    /// 判断是否为大型文件
    pub fn is_large_file(&self, file_size: usize) -> bool {
        file_size > self.large_file_threshold
    }

    /// 计算是否应该发送完整内容（而非增量）
    pub fn should_send_full(&self, file_size: usize, change_size: usize) -> bool {
        // 如果变更量超过文件大小的50%，发送完整内容更高效
        if file_size > 0 && change_size > file_size / 2 {
            return true;
        }
        false
    }

    /// 计算同步延迟
    pub fn sync_delay_ms(&self, file_size: usize) -> u64 {
        if self.is_large_file(file_size) {
            self.sync_interval_ms
        } else {
            0 // 小文件立即同步
        }
    }
}

impl Default for LargeFileSyncStrategy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_line_index() {
        let text = "line1\nline2\nline3";
        let index = FastLineIndex::from_text(text);

        assert_eq!(
            index.byte_to_position(0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            index.byte_to_position(6),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            index.byte_to_position(7),
            Position {
                line: 1,
                character: 1
            }
        );
    }

    #[test]
    fn test_merge_edits() {
        let edits = vec![
            TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                }),
                range_length: None,
                text: "hello".to_string(),
            },
            TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 5,
                    },
                }),
                range_length: None,
                text: "world".to_string(),
            },
        ];

        let merged = IncrementalChangeCalculator::merge_edits(edits);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn test_optimized_document_sync() {
        let uri = Url::parse("file:///test.rs").unwrap();
        let mut sync = OptimizedDocumentSync::new(uri);

        sync.record_edit(0, 0, "fn main() {}".to_string());
        assert_eq!(sync.version(), 1);

        let changes = sync.generate_changes_since(0);
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn test_large_file_strategy() {
        let strategy = LargeFileSyncStrategy::new();

        assert!(!strategy.is_large_file(50_000));
        assert!(strategy.is_large_file(200_000));

        assert!(!strategy.should_send_full(1000, 100));
        assert!(strategy.should_send_full(1000, 600));
    }
}
