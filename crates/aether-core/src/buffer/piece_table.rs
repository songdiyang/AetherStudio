use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;

use super::text_buffer::{BufferState, EditResult, TextBuffer, TextBufferSnapshot};

/// Piece Table — 高性能文本缓冲区
/// 支持O(1)插入/删除，零拷贝大文件打开
pub struct PieceTable {
    /// 原始文件内容（只读，内存映射，Arc共享避免快照拷贝）
    original: Option<Arc<Mmap>>,
    /// 新增内容追加缓冲区（只追加，从不删除）
    add_buffer: Vec<u8>,
    /// 有序片段表
    pieces: Vec<Piece>,
    /// 行索引：行起始位置 → 片段索引+偏移
    line_index: LineIndex,
    /// piece 起始字节偏移前缀和缓存：`piece_offset_cache[i]` = 第 i 个 piece 的起始字节偏移
    /// `piece_offset_cache[pieces.len()]` = 总字节数
    /// O(1) 替代 `byte_offset_of_piece` 的 O(n) 累积求和
    piece_offset_cache: Vec<usize>,
    /// 总字符数（UTF-8 codepoints，缓存）
    len_chars: usize,
    /// 总行数（缓存，增量更新）
    len_lines: usize,
    /// 编辑计数（用于触发碎片合并和索引重建）
    edit_count: usize,
    /// 自动合并阈值：每N次编辑后自动合并碎片
    coalesce_threshold: usize,
}

/// 一个连续片段：要么指向original，要么指向add_buffer
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub source: Source,
    pub start: usize,     // 在对应buffer中的起始字节
    pub len: usize,       // 字节长度
    pub line_breaks: u32, // 缓存：该片段中的换行符数量
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Original, // 内存映射的原始文件
    Add,      // 追加缓冲区
}

/// 行索引：每行起始字节位置
/// 支持 O(1) 行号到字节偏移转换
pub struct LineIndex {
    /// 每行起始的全局字节偏移
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new() -> Self {
        Self {
            line_starts: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.line_starts.clear();
    }

    fn push(&mut self, byte_offset: usize) {
        self.line_starts.push(byte_offset);
    }

    fn len(&self) -> usize {
        self.line_starts.len()
    }

    /// 获取指定行的起始字节偏移
    pub fn line_start(&self, line_idx: usize) -> Option<usize> {
        self.line_starts.get(line_idx).copied()
    }

    /// 获取指定行的结束字节偏移（即下一行的起始，或文本末尾）
    fn line_end(&self, line_idx: usize, total_bytes: usize) -> Option<usize> {
        if line_idx + 1 < self.line_starts.len() {
            self.line_starts.get(line_idx + 1).copied()
        } else if line_idx < self.line_starts.len() {
            Some(total_bytes)
        } else {
            None
        }
    }
}

impl PieceTable {
    /// 从字符串创建（用于新文件或测试）
    pub fn from_string(text: String) -> Self {
        let len = text.len();
        let line_breaks = count_line_breaks(text.as_bytes());
        let pieces = vec![Piece {
            source: Source::Add,
            start: 0,
            len,
            line_breaks,
        }];
        let mut pt = Self {
            original: None,
            add_buffer: text.into_bytes(),
            pieces,
            line_index: LineIndex::new(),
            piece_offset_cache: Vec::new(),
            len_chars: 0, // 简化：按字节计数
            len_lines: line_breaks as usize + 1,
            edit_count: 0,
            coalesce_threshold: 32,
        };
        pt.rebuild_line_index();
        pt
    }

    /// 从文件路径创建（使用内存映射）
    pub fn from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let len = mmap.len();
        let line_breaks = count_line_breaks(&mmap);
        let pieces = vec![Piece {
            source: Source::Original,
            start: 0,
            len,
            line_breaks,
        }];
        let mut pt = Self {
            original: Some(Arc::new(mmap)),
            add_buffer: Vec::new(),
            pieces,
            line_index: LineIndex::new(),
            piece_offset_cache: Vec::new(),
            len_chars: len,
            len_lines: line_breaks as usize + 1,
            edit_count: 0,
            coalesce_threshold: 32,
        };
        pt.rebuild_line_index();
        Ok(pt)
    }

    /// 在指定字节位置插入文本，返回受影响的行范围
    pub fn insert_with_result(&mut self, pos: usize, text: &str) -> EditResult {
        let text_bytes = text.as_bytes();
        let insert_len = text_bytes.len();
        if insert_len == 0 {
            return EditResult::default();
        }

        let total_len = self.len_bytes();
        let pos = pos.min(total_len);
        let start_line = self.byte_to_line(pos);

        // 预分配add_buffer空间，减少重新分配
        let add_start = self.add_buffer.len();
        let new_capacity = (add_start + insert_len).next_power_of_two().max(1024);
        if new_capacity > self.add_buffer.capacity() {
            self.add_buffer
                .reserve(new_capacity - self.add_buffer.capacity());
        }
        self.add_buffer.extend_from_slice(text_bytes);
        let line_breaks = count_line_breaks(text_bytes);

        if pos >= total_len && !self.pieces.is_empty() {
            self.pieces.push(Piece {
                source: Source::Add,
                start: add_start,
                len: insert_len,
                line_breaks,
            });
            self.len_chars += insert_len;
            self.len_lines += line_breaks as usize;
            self.edit_count += 1;
            self.update_line_index_for_insert(pos, text);
            if self.edit_count >= self.coalesce_threshold {
                self.coalesce_pieces();
                self.edit_count = 0;
            } else {
                self.rebuild_piece_offset_cache();
            }
            let end_line = self.len_lines.saturating_sub(1);
            return EditResult::new(start_line, end_line, line_breaks as isize);
        }

        let piece_idx = self.find_piece_at_byte(pos);
        let piece = &self.pieces[piece_idx];
        let offset_in_piece = pos - self.byte_offset_of_piece(piece_idx);

        if offset_in_piece == 0 {
            self.pieces.insert(
                piece_idx,
                Piece {
                    source: Source::Add,
                    start: add_start,
                    len: insert_len,
                    line_breaks,
                },
            );
        } else if offset_in_piece >= piece.len {
            self.pieces.insert(
                piece_idx + 1,
                Piece {
                    source: Source::Add,
                    start: add_start,
                    len: insert_len,
                    line_breaks,
                },
            );
        } else {
            let left = Piece {
                source: piece.source,
                start: piece.start,
                len: offset_in_piece,
                line_breaks: count_line_breaks_in_range(
                    self.buffer_for(piece.source),
                    piece.start,
                    offset_in_piece,
                ),
            };
            let right = Piece {
                source: piece.source,
                start: piece.start + offset_in_piece,
                len: piece.len - offset_in_piece,
                line_breaks: count_line_breaks_in_range(
                    self.buffer_for(piece.source),
                    piece.start + offset_in_piece,
                    piece.len - offset_in_piece,
                ),
            };
            let new_piece = Piece {
                source: Source::Add,
                start: add_start,
                len: insert_len,
                line_breaks,
            };
            self.pieces
                .splice(piece_idx..=piece_idx, [left, new_piece, right]);
        }

        self.len_chars += insert_len;
        self.len_lines += line_breaks as usize;
        self.edit_count += 1;
        self.update_line_index_for_insert(pos, text);
        if self.edit_count >= self.coalesce_threshold {
            self.coalesce_pieces();
            self.edit_count = 0;
        } else {
            self.rebuild_piece_offset_cache();
        }
        let end_line = (start_line + line_breaks as usize).min(self.len_lines.saturating_sub(1));
        EditResult::new(start_line, end_line, line_breaks as isize)
    }

    /// 在指定字节位置插入文本（兼容旧接口）
    pub fn insert(&mut self, pos: usize, text: &str) {
        self.insert_with_result(pos, text);
    }

    /// 删除指定字节范围 [start, end)，返回受影响的行范围
    pub fn delete_with_result(&mut self, start: usize, end: usize) -> EditResult {
        if start >= end {
            return EditResult::default();
        }

        let start_line = self.byte_to_line(start);
        let end_line_before = self.byte_to_line(end);

        let start_piece = self.find_piece_at_byte(start);
        let end_piece = self.find_piece_at_byte(end);
        let start_offset = start - self.byte_offset_of_piece(start_piece);
        let end_offset = end - self.byte_offset_of_piece(end_piece);

        if start_piece == end_piece {
            let piece = self.pieces[start_piece];
            if start_offset == 0 && end_offset == piece.len {
                self.pieces.remove(start_piece);
            } else if start_offset == 0 {
                self.pieces[start_piece] = Piece {
                    source: piece.source,
                    start: piece.start + end_offset,
                    len: piece.len - end_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(piece.source),
                        piece.start + end_offset,
                        piece.len - end_offset,
                    ),
                };
            } else if end_offset == piece.len {
                self.pieces[start_piece] = Piece {
                    source: piece.source,
                    start: piece.start,
                    len: start_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(piece.source),
                        piece.start,
                        start_offset,
                    ),
                };
            } else {
                let left = Piece {
                    source: piece.source,
                    start: piece.start,
                    len: start_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(piece.source),
                        piece.start,
                        start_offset,
                    ),
                };
                let right = Piece {
                    source: piece.source,
                    start: piece.start + end_offset,
                    len: piece.len - end_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(piece.source),
                        piece.start + end_offset,
                        piece.len - end_offset,
                    ),
                };
                self.pieces.splice(start_piece..=start_piece, [left, right]);
            }
        } else {
            let mut new_pieces = Vec::new();
            let start_p = self.pieces[start_piece];
            if start_offset > 0 {
                new_pieces.push(Piece {
                    source: start_p.source,
                    start: start_p.start,
                    len: start_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(start_p.source),
                        start_p.start,
                        start_offset,
                    ),
                });
            }
            let end_p = self.pieces[end_piece];
            if end_offset < end_p.len {
                new_pieces.push(Piece {
                    source: end_p.source,
                    start: end_p.start + end_offset,
                    len: end_p.len - end_offset,
                    line_breaks: count_line_breaks_in_range(
                        self.buffer_for(end_p.source),
                        end_p.start + end_offset,
                        end_p.len - end_offset,
                    ),
                });
            }
            self.pieces.splice(start_piece..=end_piece, new_pieces);
        }

        let old_lines = self.len_lines;
        self.len_chars = self.pieces.iter().map(|p| p.len).sum();
        self.len_lines = self
            .pieces
            .iter()
            .map(|p| p.line_breaks as usize)
            .sum::<usize>()
            + 1;
        self.edit_count += 1;
        self.update_line_index_for_delete(start, end);
        if self.edit_count >= self.coalesce_threshold {
            self.coalesce_pieces();
            self.edit_count = 0;
        } else {
            self.rebuild_piece_offset_cache();
        }
        let line_delta = self.len_lines as isize - old_lines as isize;
        let end_line = end_line_before.min(self.len_lines.saturating_sub(1));
        EditResult::new(start_line, end_line, line_delta)
    }

    /// 删除指定字节范围 [start, end)（兼容旧接口）
    pub fn delete(&mut self, start: usize, end: usize) {
        self.delete_with_result(start, end);
    }

    /// 获取总行数
    pub fn len_lines(&self) -> usize {
        self.len_lines
    }

    /// 获取总字节数
    pub fn len_bytes(&self) -> usize {
        self.pieces.iter().map(|p| p.len).sum()
    }

    /// 获取指定行的字节切片（零拷贝，性能优于 get_line）
    pub fn get_line_bytes(&self, line_idx: usize) -> Option<&[u8]> {
        let (start_byte, end_byte) = self.line_byte_range(line_idx)?;
        Some(self.get_text_bytes(start_byte, end_byte))
    }

    /// 获取指定字节范围的文本字节切片（零拷贝）
    fn get_text_bytes(&self, start: usize, end: usize) -> &[u8] {
        // 尝试找到单个piece覆盖整个范围的情况（常见场景）
        let mut current = 0;
        for piece in &self.pieces {
            let piece_end = current + piece.len;
            if current <= start && piece_end >= end {
                let buf = self.buffer_for(piece.source);
                let piece_start = piece.start + (start - current);
                let piece_end_local = piece.start + (end - current);
                return &buf[piece_start..piece_end_local];
            }
            current = piece_end;
        }
        // 跨piece情况：无法零拷贝，返回空切片（调用方应使用get_text）
        &[]
    }

    /// 获取指定行的文本（不包含换行符）
    /// 优化：优先使用零拷贝的 get_line_bytes，避免跨 piece 时的额外分配
    pub fn get_line(&self, line_idx: usize) -> Option<String> {
        let bytes = self.get_line_bytes(line_idx)?;
        if bytes.is_empty() {
            // 跨 piece 情况：回退到 get_text
            let (start_byte, end_byte) = self.line_byte_range(line_idx)?;
            let text = self.get_text(start_byte, end_byte);
            return Some(
                text.strip_suffix('\n')
                    .map(|s| s.to_string())
                    .unwrap_or(text),
            );
        }
        // 零拷贝路径：直接从 bytes 构建 String，避免 Cow 中间层
        let text = String::from_utf8_lossy(bytes);
        Some(
            text.strip_suffix('\n')
                .map(|s| s.to_string())
                .unwrap_or_else(|| text.into_owned()),
        )
    }

    /// 获取所有文本
    pub fn get_all_text(&self) -> String {
        self.get_text(0, self.len_bytes())
    }

    /// 获取 pieces 的克隆副本（用于撤销/重做快照）
    pub fn get_pieces(&self) -> Vec<Piece> {
        self.pieces.clone()
    }

    /// 获取 add_buffer 当前长度
    pub fn add_buffer_len(&self) -> usize {
        self.add_buffer.len()
    }

    /// 从历史快照恢复 pieces 状态（用于撤销/重做）
    /// 注意：add_buffer 只追加不收缩，恢复时仅替换 pieces 引用范围
    pub fn restore(&mut self, pieces: Vec<Piece>, _add_len: usize) {
        self.pieces = pieces;
        // 重新计算 len_chars 和 len_lines
        self.len_chars = self.pieces.iter().map(|p| p.len).sum();
        self.len_lines = self
            .pieces
            .iter()
            .map(|p| p.line_breaks as usize)
            .sum::<usize>()
            + 1;
        self.rebuild_line_index();
    }

    /// 获取指定字节范围的文本
    pub fn get_text(&self, start: usize, end: usize) -> String {
        let mut result = String::with_capacity(end - start);
        let mut current = 0;
        for piece in &self.pieces {
            let piece_end = current + piece.len;
            if piece_end > start && current < end {
                let piece_start = piece.start + (start.saturating_sub(current));
                let piece_end_local = piece.start + (end.min(piece_end) - current);
                let buf = self.buffer_for(piece.source);
                // 使用 lossy 转换，避免非 UTF-8 内容导致空字符串
                result.push_str(&String::from_utf8_lossy(&buf[piece_start..piece_end_local]));
            }
            current = piece_end;
        }
        result
    }

    /// 获取piece对应的buffer引用
    fn buffer_for(&self, source: Source) -> &[u8] {
        match source {
            Source::Original => self
                .original
                .as_ref()
                .map(|m| m.as_ref().as_ref())
                .unwrap_or(&[]),
            Source::Add => &self.add_buffer,
        }
    }

    /// 找到包含指定字节位置的piece索引
    fn find_piece_at_byte(&self, pos: usize) -> usize {
        let mut current = 0;
        for (i, piece) in self.pieces.iter().enumerate() {
            if current + piece.len > pos {
                return i;
            }
            current += piece.len;
        }
        self.pieces.len().saturating_sub(1)
    }

    /// 计算指定piece之前的字节偏移 —— O(1) 前缀和查找
    fn byte_offset_of_piece(&self, piece_idx: usize) -> usize {
        if !self.piece_offset_cache.is_empty() {
            // O(1) 前缀和查找
            self.piece_offset_cache[piece_idx]
        } else {
            // 回退：缓存未构建时累积求和
            self.pieces[..piece_idx].iter().map(|p| p.len).sum()
        }
    }

    /// 获取指定行的字节范围 [start, end)
    fn line_byte_range(&self, line_idx: usize) -> Option<(usize, usize)> {
        if line_idx >= self.len_lines {
            return None;
        }

        // O(1) 行索引查找
        let start = self.line_index.line_start(line_idx)?;
        let end = self.line_index.line_end(line_idx, self.len_bytes())?;
        Some((start, end))
    }

    /// 重建行索引 - 预计算每行起始字节位置
    /// 使用 SIMD 加速换行符查找，比逐字节遍历快 5-10 倍
    fn rebuild_line_index(&mut self) {
        let mut line_starts = Vec::new();
        line_starts.push(0); // 第0行从字节0开始
        let mut current_byte = 0;

        for piece in &self.pieces {
            let buf = self.buffer_for(piece.source);
            let piece_data = &buf[piece.start..piece.start + piece.len];
            // 使用 SIMD 加速的 find_byte_simd 批量查找换行符
            let mut offset = 0;
            while offset < piece_data.len() {
                match crate::simd_utils::find_byte_simd(&piece_data[offset..], b'\n') {
                    Some(pos) => {
                        let global_pos = current_byte + offset + pos;
                        line_starts.push(global_pos + 1); // 下一行起始
                        offset += pos + 1; // 跳过已找到的换行符
                    }
                    None => break,
                }
            }
            current_byte += piece.len;
        }

        self.line_index.clear();
        self.line_index.line_starts = line_starts;

        // 同步重建 piece 偏移前缀和缓存
        self.rebuild_piece_offset_cache();
    }

    /// 重建 piece 起始字节偏移前缀和缓存
    /// `piece_offset_cache[i]` = 第 i 个 piece 的起始字节偏移
    fn rebuild_piece_offset_cache(&mut self) {
        self.piece_offset_cache.clear();
        self.piece_offset_cache.reserve(self.pieces.len() + 1);
        let mut offset = 0usize;
        for piece in &self.pieces {
            self.piece_offset_cache.push(offset);
            offset += piece.len;
        }
        // 额外存储总字节数，方便后续使用
        self.piece_offset_cache.push(offset);
    }

    /// 增量更新行索引 - 在指定字节位置插入文本后更新
    /// 比全量重建快得多，适用于单次插入
    fn update_line_index_for_insert(&mut self, pos: usize, text: &str) {
        let text_bytes = text.as_bytes();
        let insert_len = text_bytes.len();
        if insert_len == 0 {
            return;
        }

        // 找到插入位置所在的行
        let insert_line = self.byte_to_line(pos);
        let line_start = self.line_index.line_start(insert_line).unwrap_or(0);

        // 计算插入位置在行内的偏移
        let offset_in_line = pos - line_start;

        // 收集插入文本中的换行位置（相对于插入位置）
        let mut new_line_offsets: Vec<usize> = Vec::new();
        for (i, byte) in text_bytes.iter().enumerate() {
            if *byte == b'\n' {
                new_line_offsets.push(offset_in_line + i + 1);
            }
        }

        // 从插入行开始，所有后续行的起始位置都需要增加 insert_len
        // 同时在该行偏移处插入新的换行点
        let mut new_line_starts =
            Vec::with_capacity(self.line_index.len() + new_line_offsets.len());

        // 复制插入行之前的所有行
        for i in 0..=insert_line {
            if let Some(start) = self.line_index.line_start(i) {
                new_line_starts.push(start);
            }
        }

        // 添加插入文本产生的新行起始位置
        let base_offset = line_start;
        for offset in &new_line_offsets {
            new_line_starts.push(base_offset + offset);
        }

        // 更新后续所有行的起始位置（增加插入长度）
        for i in (insert_line + 1)..self.line_index.len() {
            if let Some(start) = self.line_index.line_start(i) {
                new_line_starts.push(start + insert_len);
            }
        }

        self.line_index.clear();
        for start in new_line_starts {
            self.line_index.push(start);
        }
    }

    /// 增量更新行索引 - 在指定字节范围删除后更新
    fn update_line_index_for_delete(&mut self, start: usize, end: usize) {
        let delete_len = end - start;
        if delete_len == 0 {
            return;
        }

        let start_line = self.byte_to_line(start);
        let end_line = self.byte_to_line(end);
        let _start_line_start = self.line_index.line_start(start_line).unwrap_or(0);

        // 计算删除范围在起始行内的偏移
        let _start_offset = start - _start_line_start;
        let _end_offset = end - _start_line_start;

        // 计算删除范围内有多少个换行符
        let mut deleted_line_breaks = 0usize;
        for i in (start_line + 1)..=end_line {
            if let Some(line_start) = self.line_index.line_start(i) {
                if line_start >= start && line_start < end {
                    deleted_line_breaks += 1;
                }
            }
        }

        // 构建新的行索引
        let mut new_line_starts = Vec::with_capacity(self.line_index.len() - deleted_line_breaks);

        // 复制起始行及之前的行
        for i in 0..=start_line {
            if let Some(line_start) = self.line_index.line_start(i) {
                new_line_starts.push(line_start);
            }
        }

        // 处理被删除范围跨越的行的合并
        // 如果删除范围结束在下一行之前，则不需要新增行
        // 如果删除范围跨越多行，则这些行被合并为一行

        // 更新后续行的起始位置
        for i in (end_line + 1)..self.line_index.len() {
            if let Some(line_start) = self.line_index.line_start(i) {
                new_line_starts.push(line_start - delete_len);
            }
        }

        self.line_index.clear();
        for line_start in new_line_starts {
            self.line_index.push(line_start);
        }
    }
}

/// 计算字节数组中的换行符数量
fn count_line_breaks(data: &[u8]) -> u32 {
    // 使用SIMD加速的大文件处理
    if data.len() >= 64 {
        crate::simd_utils::count_newlines_simd(data)
    } else {
        data.iter().filter(|&&b| b == b'\n').count() as u32
    }
}

/// 计算指定范围内的换行符数量
fn count_line_breaks_in_range(data: &[u8], start: usize, len: usize) -> u32 {
    let end = (start + len).min(data.len());
    count_line_breaks(&data[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_string() {
        let pt = PieceTable::from_string("Hello\nWorld".to_string());
        assert_eq!(pt.len_lines(), 2);
        assert_eq!(pt.get_line(0), Some("Hello".to_string()));
        assert_eq!(pt.get_line(1), Some("World".to_string()));
    }

    #[test]
    fn test_insert() {
        let mut pt = PieceTable::from_string("Hello World".to_string());
        pt.insert(6, "Beautiful ");
        assert_eq!(pt.get_all_text(), "Hello Beautiful World");
    }

    #[test]
    fn test_delete() {
        let mut pt = PieceTable::from_string("Hello Beautiful World".to_string());
        pt.delete(6, 16);
        assert_eq!(pt.get_all_text(), "Hello World");
    }

    #[test]
    fn test_insert_at_boundaries() {
        let mut pt = PieceTable::from_string("AB".to_string());
        pt.insert(0, "X");
        pt.insert(4, "Y");
        assert_eq!(pt.get_all_text(), "XABY");
    }

    #[test]
    fn test_multiple_edits() {
        let mut pt = PieceTable::from_string("".to_string());
        for i in 0..1000 {
            pt.insert(pt.len_bytes(), &format!("line {}\n", i));
        }
        assert_eq!(pt.len_lines(), 1001);
    }
}

// ============================================================================
// TextBuffer trait 实现
// ============================================================================

/// PieceTable 不可变快照
/// 包含 piece 列表的副本和 buffer 引用（通过 Arc 共享）
pub struct PieceTableSnapshot {
    pieces: Vec<Piece>,
    add_buffer: Arc<Vec<u8>>,
    original: Option<Arc<Mmap>>, // 零拷贝：直接共享 Arc<Mmap>，避免大文件内存拷贝
    len_lines: usize,
}

impl TextBufferSnapshot for PieceTableSnapshot {
    fn slice(&self, start: usize, end: usize) -> String {
        let mut result = String::with_capacity(end - start);
        let mut current = 0;
        for piece in &self.pieces {
            let piece_end = current + piece.len;
            if piece_end > start && current < end {
                let piece_start = piece.start + (start.saturating_sub(current));
                let piece_end_local = piece.start + (end.min(piece_end) - current);
                let buf = self.buffer_for(piece.source);
                result.push_str(&String::from_utf8_lossy(&buf[piece_start..piece_end_local]));
            }
            current = piece_end;
        }
        result
    }

    fn full_text(&self) -> String {
        self.slice(0, self.byte_len())
    }

    fn line_count(&self) -> usize {
        self.len_lines
    }

    fn line_text(&self, line_idx: usize) -> Option<String> {
        let (start_byte, end_byte) = self.line_byte_range(line_idx)?;
        Some(self.slice(start_byte, end_byte))
    }

    fn byte_len(&self) -> usize {
        self.pieces.iter().map(|p| p.len).sum()
    }
}

impl PieceTableSnapshot {
    fn buffer_for(&self, source: Source) -> &[u8] {
        match source {
            Source::Original => self
                .original
                .as_ref()
                .map(|m| m.as_ref().as_ref())
                .unwrap_or(&[]),
            Source::Add => &self.add_buffer,
        }
    }

    fn line_byte_range(&self, line_idx: usize) -> Option<(usize, usize)> {
        if line_idx >= self.len_lines {
            return None;
        }
        let mut current_line = 0;
        let mut line_start = 0;
        let mut current_byte = 0;

        for piece in &self.pieces {
            let buf = self.buffer_for(piece.source);
            let piece_data = &buf[piece.start..piece.start + piece.len];
            for (i, byte) in piece_data.iter().enumerate() {
                let global_byte = current_byte + i;
                if *byte == b'\n' {
                    if current_line == line_idx {
                        return Some((line_start, global_byte));
                    }
                    current_line += 1;
                    line_start = global_byte + 1;
                }
            }
            current_byte += piece.len;
        }

        if current_line == line_idx {
            Some((line_start, current_byte))
        } else {
            None
        }
    }
}

impl TextBuffer for PieceTable {
    fn insert(&mut self, pos: usize, text: &str) {
        self.insert(pos, text);
    }

    fn delete(&mut self, start: usize, end: usize) {
        self.delete(start, end);
    }

    fn slice(&self, start: usize, end: usize) -> String {
        self.get_text(start, end)
    }

    fn full_text(&self) -> String {
        self.get_all_text()
    }

    fn line_count(&self) -> usize {
        self.len_lines()
    }

    fn byte_len(&self) -> usize {
        self.len_bytes()
    }

    fn line_text(&self, line_idx: usize) -> Option<String> {
        self.get_line(line_idx)
    }

    fn line_byte_range(&self, line_idx: usize) -> Option<(usize, usize)> {
        self.line_byte_range(line_idx)
    }

    fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let mut pos = 0;
        for i in 0..line {
            if let Some(text) = self.get_line(i) {
                pos += text.len() + 1; // +1 for '\n'
            }
        }
        if let Some(text) = self.get_line(line) {
            pos + col.min(text.len())
        } else {
            pos
        }
    }

    fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        // 使用行索引二分查找：O(log n) 替代 O(n) 逐字节遍历
        let byte = byte.min(self.len_bytes().saturating_sub(1));
        match self.line_index.line_starts.binary_search(&byte) {
            Ok(idx) => {
                // 恰好是某行起始位置
                (idx, 0)
            }
            Err(idx) => {
                // idx 是 byte 应该插入的位置，即 byte 所在行号
                let line = idx.saturating_sub(1);
                let line_start = self.line_index.line_start(line).unwrap_or(0);
                (line, byte - line_start)
            }
        }
    }

    fn create_snapshot(&self) -> Box<dyn TextBufferSnapshot> {
        // 零拷贝快照：直接克隆 Arc<Mmap>，共享内存映射引用
        // 避免大文件的全量内存拷贝，显著提升打开文件性能
        let original = self.original.as_ref().map(|arc_mmap| arc_mmap.clone());
        Box::new(PieceTableSnapshot {
            pieces: self.pieces.clone(),
            add_buffer: Arc::new(self.add_buffer.clone()),
            original,
            len_lines: self.len_lines,
        })
    }

    fn save_state(&self) -> BufferState {
        // 序列化 piece 元数据
        let mut pieces_data = Vec::with_capacity(self.pieces.len() * 16);
        for piece in &self.pieces {
            pieces_data.extend_from_slice(&(piece.source as u32).to_le_bytes());
            pieces_data.extend_from_slice(&piece.start.to_le_bytes());
            pieces_data.extend_from_slice(&piece.len.to_le_bytes());
            pieces_data.extend_from_slice(&piece.line_breaks.to_le_bytes());
        }
        BufferState {
            pieces_data,
            add_buffer_len: self.add_buffer.len(),
            line_count: self.len_lines,
            byte_len: self.len_bytes(),
        }
    }

    fn restore_state(&mut self, state: BufferState) {
        // 反序列化 piece 元数据
        let piece_size = 16; // 4 * 4 bytes
        let piece_count = state.pieces_data.len() / piece_size;
        let mut pieces = Vec::with_capacity(piece_count);
        for i in 0..piece_count {
            let offset = i * piece_size;
            let source = u32::from_le_bytes([
                state.pieces_data[offset],
                state.pieces_data[offset + 1],
                state.pieces_data[offset + 2],
                state.pieces_data[offset + 3],
            ]);
            let start = u32::from_le_bytes([
                state.pieces_data[offset + 4],
                state.pieces_data[offset + 5],
                state.pieces_data[offset + 6],
                state.pieces_data[offset + 7],
            ]) as usize;
            let len = u32::from_le_bytes([
                state.pieces_data[offset + 8],
                state.pieces_data[offset + 9],
                state.pieces_data[offset + 10],
                state.pieces_data[offset + 11],
            ]) as usize;
            let line_breaks = u32::from_le_bytes([
                state.pieces_data[offset + 12],
                state.pieces_data[offset + 13],
                state.pieces_data[offset + 14],
                state.pieces_data[offset + 15],
            ]);
            pieces.push(Piece {
                source: if source == 0 {
                    Source::Original
                } else {
                    Source::Add
                },
                start,
                len,
                line_breaks,
            });
        }
        self.pieces = pieces;
        self.len_lines = state.line_count;
        self.len_chars = state.byte_len;
        self.rebuild_line_index();
    }
}

impl PieceTable {
    /// 获取指定行的起始字节偏移 - O(1)
    pub fn line_start_byte(&self, line: usize) -> usize {
        self.line_index.line_start(line).unwrap_or(0)
    }

    /// 将字节偏移转换为行号 - O(log n) 二分查找
    fn byte_to_line(&self, byte: usize) -> usize {
        // 使用行索引二分查找
        match self.line_index.line_starts.binary_search(&byte) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        }
    }

    /// 合并相邻的同 Source piece，减少碎片
    fn coalesce_pieces(&mut self) {
        if self.pieces.len() < 2 {
            return;
        }

        let mut i = 0;
        while i + 1 < self.pieces.len() {
            let current = self.pieces[i];
            let next = self.pieces[i + 1];

            // 只有当两个piece都是Add且连续时才能合并
            // Original piece 不能合并，因为它们是内存映射的引用
            if current.source == Source::Add
                && next.source == Source::Add
                && current.start + current.len == next.start
            {
                let merged = Piece {
                    source: Source::Add,
                    start: current.start,
                    len: current.len + next.len,
                    line_breaks: current.line_breaks + next.line_breaks,
                };
                self.pieces[i] = merged;
                self.pieces.remove(i + 1);
                // 不递增 i，继续检查是否可以继续合并
            } else {
                i += 1;
            }
        }

        // 合并后重建前缀和缓存
        self.rebuild_piece_offset_cache();
    }

    /// 延迟重建行索引 - 批量编辑时减少重建次数
    /// 返回 true 表示需要重建
    #[allow(dead_code)]
    fn needs_rebuild(&self, _edit_count: usize) -> bool {
        // 简单策略：总是重建（可以改为计数器策略）
        true
    }
}
