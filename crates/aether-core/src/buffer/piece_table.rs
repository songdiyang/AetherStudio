#![allow(clippy::items_after_test_module)]

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

    fn len(&self) -> usize {
        self.line_starts.len()
    }

    /// 获取指定行的起始字节偏移
    pub fn line_start(&self, line_idx: usize) -> Option<usize> {
        self.line_starts.get(line_idx).copied()
    }

    /// 在指定位置插入新的行起始偏移，O(K + N) 其中 K=new_starts.len(), N=尾部移动量
    /// 比重建整个 Vec 高效：避免重新分配和前半部分复制
    fn splice_insert(&mut self, insert_at: usize, new_starts: Vec<usize>) {
        self.line_starts.splice(insert_at..insert_at, new_starts);
    }

    /// 从指定行开始，所有行起始偏移加上 delta（增量调整，O(N-tail)）
    fn shift_from(&mut self, from_line: usize, delta: usize) {
        for start in &mut self.line_starts[from_line..] {
            *start += delta;
        }
    }

    /// 删除指定行范围的起始偏移 [start_line, end_line)
    fn drain_range(&mut self, start_line: usize, end_line: usize) {
        if start_line < end_line && end_line <= self.line_starts.len() {
            self.line_starts.drain(start_line..end_line);
        }
    }

    /// 从指定行开始，所有行起始偏移减去 delta（用于删除后调整）
    fn shift_from_sub(&mut self, from_line: usize, delta: usize) {
        for start in &mut self.line_starts[from_line..] {
            *start -= delta;
        }
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

        // C-01: 允许空表走此分支（pos>=total_len 且 pieces 为空时直接 push 新 piece），
        // 否则空表插入会落到 find_piece_at_byte -> pieces[0] 越界 panic
        if pos >= total_len {
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
        // C-19: 边界钳位，防止 end 超出缓冲区长度导致数据损坏
        let end = end.min(self.len_bytes());
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
        // C-03: 在修改 pieces 前已计算 end_line，避免使用修改后的状态
        self.update_line_index_for_delete(start, end, end_line_before);
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

    /// 获取总字节数 — CORE-H01: O(1) 使用前缀和缓存
    pub fn len_bytes(&self) -> usize {
        if !self.piece_offset_cache.is_empty() {
            // piece_offset_cache 最后一个元素是总字节数
            *self.piece_offset_cache.last().unwrap_or(&0)
        } else {
            self.pieces.iter().map(|p| p.len).sum()
        }
    }

    /// 获取指定行的字节切片（零拷贝，性能优于 get_line）
    /// 返回 None 表示行不存在或该行跨越多个 piece（需改用 get_line 获取拼接结果）
    pub fn get_line_bytes(&self, line_idx: usize) -> Option<&[u8]> {
        let (start_byte, end_byte) = self.line_byte_range(line_idx)?;
        self.get_text_bytes(start_byte, end_byte)
    }

    /// 获取指定字节范围的文本字节切片（零拷贝）
    /// C-03: 返回 Option 区分"单 piece 命中（含空切片=空行）"与"跨 piece 无法零拷贝"。
    /// 跨 piece 时返回 None，调用方应回退到 get_text 拼接，避免静默返回空数据。
    ///
    /// 性能：使用 piece_offset_cache 做二分查找定位起始 piece，O(log n)。
    /// 早期实现线性扫描 pieces，编辑后 pieces 增多时（coalesce_threshold=32
    /// 才合并一次），get_line 渲染 1000 行会累积 30000+ 次扫描。
    fn get_text_bytes(&self, start: usize, end: usize) -> Option<&[u8]> {
        if start >= end {
            return Some(&[]);
        }
        // 找到覆盖 start 的 piece（使用二分查找）
        let piece_idx = self.find_piece_at_byte(start);
        let piece = self.pieces.get(piece_idx)?;
        let piece_offset = self.byte_offset_of_piece(piece_idx);
        // 单 piece 覆盖整个 [start, end)？
        if piece_offset + piece.len >= end {
            let buf = self.buffer_for(piece.source);
            let byte_start_in_buf = piece.start + (start - piece_offset);
            let byte_end_in_buf = piece.start + (end - piece_offset);
            return Some(&buf[byte_start_in_buf..byte_end_in_buf]);
        }
        // 跨 piece：无法零拷贝，返回 None（调用方回退到 get_text）
        None
    }

    /// 获取指定行的文本（不包含换行符）
    /// 优化：优先使用零拷贝的 get_line_bytes，避免跨 piece 时的额外分配
    pub fn get_line(&self, line_idx: usize) -> Option<String> {
        // C-03: get_line_bytes 返回 None 表示行不存在或跨 piece；
        // 行不存在时 line_byte_range 已返回 None，这里 None 表示跨 piece
        let (start_byte, end_byte) = self.line_byte_range(line_idx)?;
        match self.get_text_bytes(start_byte, end_byte) {
            // 单 piece 命中：零拷贝路径
            Some(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                Some(
                    text.strip_suffix('\n')
                        .map(|s| s.strip_suffix('\r').unwrap_or(s).to_string())
                        .unwrap_or_else(|| text.into_owned()),
                )
            }
            // C-03: 跨 piece 时回退到 get_text 拼接，避免静默返回空数据
            None => {
                let text = self.get_text(start_byte, end_byte);
                Some(
                    text.strip_suffix('\n')
                        .map(|s| s.strip_suffix('\r').unwrap_or(s).to_string())
                        .unwrap_or(text),
                )
            }
        }
    }

    /// 获取所有文本
    pub fn get_all_text(&self) -> String {
        self.get_text(0, self.len_bytes())
    }

    /// 将缓冲区全部内容直接写入 writer，避免 get_all_text 的中间 String 分配。
    /// 性能：对未编辑的大文件（original 是 mmap），每个 piece 直接写出 &[u8] 切片，
    /// 无堆分配；编辑后的文件也只是多次 write_all，仍避免了一次全量 String 拼接。
    pub fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        for piece in &self.pieces {
            let buf = self.buffer_for(piece.source);
            let end = piece.start.checked_add(piece.len).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "piece start+len 溢出")
            })?;
            if end > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "piece 引用超出 buffer 边界",
                ));
            }
            writer.write_all(&buf[piece.start..end])?;
        }
        Ok(())
    }

    /// 判断缓冲区是否未经过编辑（pieces 仅引用 original buffer）。
    /// 用于保存文件时判断是否可以直接从 mmap 零拷贝写入磁盘。
    pub fn is_pristine(&self) -> bool {
        self.pieces.iter().all(|p| p.source == Source::Original)
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

    /// P4-1: 获取指定字节位置的单个字节（零拷贝，无堆分配）
    ///
    /// 相比 `get_text(p, p+1).as_bytes()[0]` 避免了 String 堆分配和
    /// UTF-8 lossy 转换开销。利用 `find_piece_at_byte` 的二分查找，O(log n)。
    /// 用于 `find_prev_char_boundary` / `find_next_char_boundary` 等逐字节扫描场景。
    pub fn byte_at(&self, pos: usize) -> Option<u8> {
        if pos >= self.len_bytes() {
            return None;
        }
        // 空 buffer：直接返回 None
        if self.pieces.is_empty() {
            return None;
        }
        let piece_idx = self.find_piece_at_byte(pos);
        let piece = self.pieces.get(piece_idx)?;
        let offset_in_piece = pos.saturating_sub(self.byte_offset_of_piece(piece_idx));
        // 边界保护：偏移必须落在当前 piece 内
        if offset_in_piece >= piece.len {
            return None;
        }
        let buf = self.buffer_for(piece.source);
        buf.get(piece.start + offset_in_piece).copied()
    }

    /// 找到包含指定字节位置的piece索引
    /// P4-3: 优先使用 piece_offset_cache 做二分查找 O(log n)；
    /// 缓存未构建时回退到线性扫描 O(n)。
    fn find_piece_at_byte(&self, pos: usize) -> usize {
        // CORE-C02: 空 piece table 返回 0 而非越界索引
        if self.pieces.is_empty() {
            return 0;
        }
        // P4-3: 使用前缀和缓存做二分查找
        if !self.piece_offset_cache.is_empty() {
            // piece_offset_cache 末尾存有总字节数（哨兵），整体仍保持升序
            match self.piece_offset_cache.binary_search(&pos) {
                Ok(idx) => {
                    // pos 恰好等于某 piece 起点；若 idx == pieces.len() 表示 pos == 总字节数
                    if idx >= self.pieces.len() {
                        return self.pieces.len() - 1;
                    }
                    idx
                }
                Err(idx) => {
                    // pos 落在 piece_offset_cache[idx-1] 与 piece_offset_cache[idx] 之间
                    if idx == 0 {
                        0
                    } else {
                        // idx-1 不可能超过 pieces.len()-1，因为 piece_offset_cache 长度 = pieces.len()+1
                        (idx - 1).min(self.pieces.len() - 1)
                    }
                }
            }
        } else {
            // 回退：缓存未构建时线性扫描
            let mut current = 0;
            for (i, piece) in self.pieces.iter().enumerate() {
                if current + piece.len > pos {
                    return i;
                }
                current += piece.len;
            }
            self.pieces.len().saturating_sub(1)
        }
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

        // 收集插入文本产生的新行起始位置（绝对字节偏移）
        // 新行从每个 '\n' 后一个字节开始
        let mut new_line_starts: Vec<usize> = Vec::new();
        for (i, &byte) in text_bytes.iter().enumerate() {
            if byte == b'\n' {
                new_line_starts.push(pos + i + 1);
            }
        }

        // 真正的增量更新：避免重建整个 Vec
        // 1. 从 insert_line+1 开始，后续行的起始位置 += insert_len
        if insert_line + 1 < self.line_index.len() {
            self.line_index.shift_from(insert_line + 1, insert_len);
        }

        // 2. 在 insert_line 之后插入新行（splice 原地插入，无需重新分配前半部分）
        if !new_line_starts.is_empty() {
            self.line_index
                .splice_insert(insert_line + 1, new_line_starts);
        }
    }

    /// 增量更新行索引 - 在指定字节范围删除后更新
    /// `end_line` 必须在修改 pieces 前计算好，避免使用已损坏的 byte 映射
    fn update_line_index_for_delete(&mut self, start: usize, end: usize, end_line: usize) {
        let delete_len = end - start;
        if delete_len == 0 {
            return;
        }

        let start_line = self.byte_to_line(start);

        // 确定需要删除的行范围 [start_line+1, drain_end)
        // start_line+1..end_line 的行起始必在 [start, end) 内，必删
        // end_line 行起始若 <= end-1（即位于 end 之前的换行符产生的行起点）则该行也应被删除；
        // H-02: 原条件 ls < end 漏删了 ls == end 的边界情况（该行起点由 end-1 处的 '\n' 产生），
        // 导致删除区间恰好结束于某行起点前的换行符时残留幽灵行起点。改为 ls <= end。
        let drain_end = if end_line > start_line {
            match self.line_index.line_start(end_line) {
                Some(ls) if ls <= end => end_line + 1,
                _ => end_line,
            }
        } else {
            start_line + 1
        };

        // 真正的增量更新：
        // 1. 删除 [start_line+1, drain_end) 范围的行起始
        if start_line + 1 < drain_end {
            self.line_index.drain_range(start_line + 1, drain_end);
        }

        // 2. 从 start_line+1 开始，后续所有行起始 -= delete_len
        if start_line + 1 < self.line_index.len() {
            self.line_index.shift_from_sub(start_line + 1, delete_len);
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

    #[test]
    fn test_delete_line_index_consistency() {
        // C-03: 删除跨行文本后，行索引应与重新构建一致
        let mut pt = PieceTable::from_string("line1\nline2\nline3\nline4\n".to_string());

        // 先插入一些文本，制造多个 piece
        pt.insert(6, "inserted\n"); // "line1\ninserted\nline2\n..."
        assert_eq!(pt.len_lines(), 6);

        // 删除从 "inserted" 中间到 "line3" 中间的内容
        // 位置：line1\n(7) i(8) n(9) ... inserted\n(17) line2\n(23) l(24) i(25) n(26) e(27) 3(28) \n(29) line4...
        let start = 10; // "inserted\n" 内部
        let end = 26; // "line3" 内部
        pt.delete(start, end);

        // 重新构建行索引作为参考
        let pt_rebuild = PieceTable::from_string(pt.get_all_text());
        assert_eq!(pt.len_lines(), pt_rebuild.len_lines());
        for i in 0..pt.len_lines() {
            assert_eq!(
                pt.get_line(i),
                pt_rebuild.get_line(i),
                "line {} mismatch",
                i
            );
        }
    }

    #[test]
    fn test_empty_piece_table() {
        let pt = PieceTable::from_string("".to_string());
        assert_eq!(pt.len_bytes(), 0);
        assert_eq!(pt.len_lines(), 1);
        assert_eq!(pt.get_line(0), Some("".to_string()));
        assert_eq!(pt.get_line(1), None);
    }

    #[test]
    fn test_insert_at_end_and_beginning() {
        let mut pt = PieceTable::from_string("middle".to_string());
        pt.insert(0, "start ");
        pt.insert(pt.len_bytes(), " end");
        assert_eq!(pt.get_all_text(), "start middle end");
    }

    #[test]
    fn test_insert_middle_of_piece() {
        let mut pt = PieceTable::from_string("abcdef".to_string());
        pt.insert(3, "XYZ");
        assert_eq!(pt.get_all_text(), "abcXYZdef");
        // 跨 piece 边界读取
        assert_eq!(pt.get_line(0), Some("abcXYZdef".to_string()));
    }

    #[test]
    fn test_delete_whole_and_partial_piece() {
        let mut pt = PieceTable::from_string("hello world".to_string());
        pt.delete(0, 5);
        assert_eq!(pt.get_all_text(), " world");
        pt.delete(1, 3);
        assert_eq!(pt.get_all_text(), " rld");
    }

    #[test]
    fn test_delete_across_multiple_pieces() {
        let mut pt = PieceTable::from_string("abcdef".to_string());
        pt.insert(3, "XYZ"); // abcXYZdef
        pt.insert(6, "123"); // abcXYZ123def
                             // 删除从第1个 piece 到第3个 piece 的部分内容 [1,9)
        pt.delete(1, 9);
        assert_eq!(pt.get_all_text(), "adef");
    }

    #[test]
    fn test_insert_and_delete_empty() {
        let mut pt = PieceTable::from_string("abc".to_string());
        pt.insert(1, "");
        assert_eq!(pt.get_all_text(), "abc");
        pt.delete(1, 1);
        assert_eq!(pt.get_all_text(), "abc");
    }

    #[test]
    fn test_delete_beyond_end_clamped() {
        let mut pt = PieceTable::from_string("abc".to_string());
        pt.delete(1, 100);
        assert_eq!(pt.get_all_text(), "a");
    }

    #[test]
    fn test_crlf_line_endings() {
        let pt = PieceTable::from_string("line1\r\nline2\r\n".to_string());
        assert_eq!(pt.len_lines(), 3);
        assert_eq!(pt.get_line(0), Some("line1".to_string()));
        assert_eq!(pt.get_line(1), Some("line2".to_string()));
    }

    #[test]
    fn test_byte_at() {
        let mut pt = PieceTable::from_string("abcdef".to_string());
        assert_eq!(pt.byte_at(0), Some(b'a'));
        assert_eq!(pt.byte_at(5), Some(b'f'));
        assert_eq!(pt.byte_at(6), None);
        pt.insert(3, "XYZ");
        assert_eq!(pt.byte_at(3), Some(b'X'));
        assert_eq!(pt.byte_at(5), Some(b'Z'));
        assert_eq!(pt.byte_at(6), Some(b'd'));
    }

    #[test]
    fn test_get_text_bytes_and_cross_piece_fallback() {
        let mut pt = PieceTable::from_string("abcdef".to_string());
        pt.insert(3, "XYZ");
        // 插入后第0行跨多个 piece，零拷贝路径应返回 None，走 get_text 拼接
        let line_bytes = pt.get_line_bytes(0);
        assert!(line_bytes.is_none());
        assert_eq!(pt.get_text(2, 7), "cXYZd");
    }

    #[test]
    fn test_restore_and_snapshot() {
        let mut pt = PieceTable::from_string("abcdef".to_string());
        let pieces = pt.get_pieces();
        let add_len = pt.add_buffer_len();
        pt.insert(3, "XYZ");
        assert_ne!(pt.get_all_text(), "abcdef");
        pt.restore(pieces, add_len);
        assert_eq!(pt.get_all_text(), "abcdef");

        let snapshot = pt.create_snapshot();
        assert_eq!(snapshot.full_text(), "abcdef");
        assert_eq!(snapshot.byte_len(), 6);
    }

    #[test]
    fn test_save_restore_state() {
        let mut pt = PieceTable::from_string("abc\ndef\n".to_string());
        let state = pt.save_state();
        pt.insert(3, "XYZ");
        assert_ne!(pt.get_all_text(), "abc\ndef\n");
        pt.restore_state(state);
        assert_eq!(pt.get_all_text(), "abc\ndef\n");
        assert_eq!(pt.len_lines(), 3);
    }

    #[test]
    fn test_get_line_bytes_single_piece() {
        let pt = PieceTable::from_string("single line".to_string());
        assert_eq!(pt.get_line_bytes(0), Some("single line".as_bytes()));
    }

    #[test]
    fn test_line_col_to_byte_and_byte_to_line_col() {
        let pt = PieceTable::from_string("abc\ndefgh\nij".to_string());
        assert_eq!(pt.line_col_to_byte(0, 2), 2);
        assert_eq!(pt.line_col_to_byte(1, 3), 7);
        assert_eq!(pt.line_col_to_byte(2, 10), pt.len_bytes());
        assert_eq!(pt.byte_to_line_col(0), (0, 0));
        assert_eq!(pt.byte_to_line_col(4), (1, 0));
        assert_eq!(pt.byte_to_line_col(8), (1, 4));
        assert_eq!(pt.byte_to_line_col(pt.len_bytes()), (2, 2));
    }

    #[test]
    fn test_insert_with_result_line_delta() {
        let mut pt = PieceTable::from_string("abc\ndef".to_string());
        let result = pt.insert_with_result(3, "\nxyz\n");
        assert_eq!(result.line_delta, 2);
        assert_eq!(pt.len_lines(), 4);
    }

    #[test]
    fn test_delete_with_result_line_delta() {
        let mut pt = PieceTable::from_string("a\nb\nc\nd".to_string());
        let result = pt.delete_with_result(1, 5);
        assert_eq!(result.line_delta, -2);
        assert_eq!(pt.get_all_text(), "a\nd");
    }

    #[test]
    fn test_coalesce_threshold() {
        let mut pt = PieceTable::from_string("".to_string());
        for i in 0..50 {
            pt.insert(pt.len_bytes(), &format!("x{}", i));
        }
        // 合并后 piece 数量应远小于 50
        assert!(pt.get_pieces().len() < 50);
    }

    #[test]
    fn test_large_file_lines() {
        let mut text = String::new();
        for i in 0..1000 {
            text.push_str(&format!("line {}\n", i));
        }
        let pt = PieceTable::from_string(text);
        assert_eq!(pt.len_lines(), 1001);
        assert_eq!(pt.get_line(0), Some("line 0".to_string()));
        assert_eq!(pt.get_line(999), Some("line 999".to_string()));
    }

    #[test]
    fn test_text_buffer_trait_methods() {
        use super::super::text_buffer::TextBuffer;
        let mut pt = PieceTable::from_string("hello\nworld".to_string());
        assert_eq!(pt.byte_len(), 11);
        assert_eq!(pt.line_count(), 2);
        assert_eq!(pt.line_text(1), Some("world".to_string()));
        assert_eq!(pt.slice(0, 5), "hello");
        assert_eq!(pt.full_text(), "hello\nworld");
        pt.insert(5, "!");
        pt.delete(0, 1);
        assert_eq!(pt.full_text(), "ello!\nworld");
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
        // C-20: 使用 line_index 实现 O(1) 查找，替代 O(n) 逐行扫描
        let line_start = self.line_index.line_start(line).unwrap_or(0);
        let line_end = if line + 1 < self.line_index.line_starts.len() {
            self.line_index.line_starts[line + 1]
        } else {
            self.len_bytes()
        };
        // 行长度（不含换行符）
        let line_len = if line_end > line_start {
            // 减去换行符字节（1 或 2 字节 CRLF）
            let raw_len = line_end - line_start;
            if raw_len >= 2 {
                let text = self.get_text(line_start, line_end);
                if text.ends_with("\r\n") {
                    raw_len - 2
                } else if text.ends_with('\n') {
                    raw_len - 1
                } else {
                    raw_len
                }
            } else if raw_len == 1 {
                let text = self.get_text(line_start, line_end);
                if text.ends_with('\n') {
                    0
                } else {
                    1
                }
            } else {
                0
            }
        } else {
            0
        };
        line_start + col.min(line_len)
    }

    fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        let total_bytes = self.len_bytes();
        // CORE-C03: 缓冲区末尾光标是合法位置，不应被 clamp 到上一行
        if byte >= total_bytes {
            // 光标在文本末尾之后，返回最后一行末尾位置
            let last_line = self.len_lines.saturating_sub(1);
            let line_start = self.line_index.line_start(last_line).unwrap_or(0);
            return (last_line, total_bytes.saturating_sub(line_start));
        }
        match self.line_index.line_starts.binary_search(&byte) {
            Ok(idx) => (idx, 0),
            Err(idx) => {
                let line = idx.saturating_sub(1);
                let line_start = self.line_index.line_start(line).unwrap_or(0);
                (line, byte - line_start)
            }
        }
    }

    fn create_snapshot(&self) -> Box<dyn TextBufferSnapshot> {
        // 零拷贝快照：直接克隆 Arc<Mmap>，共享内存映射引用
        // 避免大文件的全量内存拷贝，显著提升打开文件性能
        // CORE-M01: add_buffer 仍需克隆（待改为 Arc<Vec<u8>> 实现真正零拷贝）
        let original = self.original.as_ref().map(|arc_mmap| arc_mmap.clone());
        Box::new(PieceTableSnapshot {
            pieces: self.pieces.clone(),
            add_buffer: Arc::new(self.add_buffer.clone()),
            original,
            len_lines: self.len_lines,
        })
    }

    fn save_state(&self) -> BufferState {
        // 序列化 piece 元数据 — CORE-H02: 使用 u64 防止大文件偏移截断
        let mut pieces_data = Vec::with_capacity(self.pieces.len() * 24);
        for piece in &self.pieces {
            pieces_data.extend_from_slice(&(piece.source as u64).to_le_bytes());
            pieces_data.extend_from_slice(&(piece.start as u64).to_le_bytes());
            pieces_data.extend_from_slice(&(piece.len as u64).to_le_bytes());
            pieces_data.extend_from_slice(&(piece.line_breaks as u64).to_le_bytes());
        }
        BufferState {
            pieces_data,
            add_buffer_len: self.add_buffer.len(),
            line_count: self.len_lines,
            byte_len: self.len_bytes(),
        }
    }

    fn restore_state(&mut self, state: BufferState) {
        // H-19: 反序列化前完整校验，任何字段越界/损坏都放弃恢复，保留当前状态
        // 避免后续 piece 切片访问触发 OOB panic 或读取未初始化内存
        match self.restore_state_checked(state) {
            Ok(()) => {}
            Err(msg) => {
                eprintln!("[ERROR] restore_state 校验失败，放弃恢复: {}", msg);
            }
        }
    }
}

impl PieceTable {
    /// H-19: 带边界校验的状态恢复。
    /// 对反序列化后的每个 piece 严格校验 source/start/len/line_breaks，
    /// 任何字段越界或损坏均返回 Err，调用方可选择保留旧状态。
    pub fn restore_state_checked(&mut self, state: BufferState) -> Result<(), String> {
        const PIECE_SIZE: usize = 32; // 8 * 4 bytes

        // 1) pieces_data 长度必须是 PIECE_SIZE 的整数倍，否则字节流已损坏
        if state.pieces_data.len() % PIECE_SIZE != 0 {
            return Err(format!(
                "pieces_data 长度 {} 不是 {} 的整数倍",
                state.pieces_data.len(),
                PIECE_SIZE
            ));
        }

        let piece_count = state.pieces_data.len() / PIECE_SIZE;
        let mut pieces = Vec::with_capacity(piece_count);

        // 缓存 original buffer 长度（若存在）以便逐 piece 校验 Source::Original 的边界
        let original_len = self.original.as_ref().map(|m| m.len()).unwrap_or(0usize);
        let add_len = self.add_buffer.len();

        // 2) 交叉校验 add_buffer 长度：add_buffer 只追加不收缩，
        //    保存时的长度必须 <= 当前长度，否则说明 add_buffer 已被异常截断
        if state.add_buffer_len > add_len {
            return Err(format!(
                "add_buffer_len 异常: state={} current={}（add_buffer 不应收缩）",
                state.add_buffer_len, add_len
            ));
        }

        let mut total_bytes: u64 = 0;

        for i in 0..piece_count {
            let offset = i * PIECE_SIZE;
            let src_raw = u64::from_le_bytes([
                state.pieces_data[offset],
                state.pieces_data[offset + 1],
                state.pieces_data[offset + 2],
                state.pieces_data[offset + 3],
                state.pieces_data[offset + 4],
                state.pieces_data[offset + 5],
                state.pieces_data[offset + 6],
                state.pieces_data[offset + 7],
            ]);
            // source 必须是 0 (Original) 或 1 (Add)，其它值视为损坏
            if src_raw > 1 {
                return Err(format!("piece {} source 非法值: {}", i, src_raw));
            }
            let source = if src_raw == 0 {
                Source::Original
            } else {
                Source::Add
            };

            let start = u64::from_le_bytes([
                state.pieces_data[offset + 8],
                state.pieces_data[offset + 9],
                state.pieces_data[offset + 10],
                state.pieces_data[offset + 11],
                state.pieces_data[offset + 12],
                state.pieces_data[offset + 13],
                state.pieces_data[offset + 14],
                state.pieces_data[offset + 15],
            ]);
            let len = u64::from_le_bytes([
                state.pieces_data[offset + 16],
                state.pieces_data[offset + 17],
                state.pieces_data[offset + 18],
                state.pieces_data[offset + 19],
                state.pieces_data[offset + 20],
                state.pieces_data[offset + 21],
                state.pieces_data[offset + 22],
                state.pieces_data[offset + 23],
            ]);
            let line_breaks = u64::from_le_bytes([
                state.pieces_data[offset + 24],
                state.pieces_data[offset + 25],
                state.pieces_data[offset + 26],
                state.pieces_data[offset + 27],
                state.pieces_data[offset + 28],
                state.pieces_data[offset + 29],
                state.pieces_data[offset + 30],
                state.pieces_data[offset + 31],
            ]);

            // start + len 不能溢出 usize（在 64-bit 上 u64 直接转 usize不会溢出，但32-bit可能）
            let start_us = start as usize;
            let len_us = len as usize;
            let lb_us = line_breaks as u32;

            // 3) start + len 不能溢出
            let end = start_us
                .checked_add(len_us)
                .ok_or_else(|| format!("piece {} start+len 溢出: {}+{}", i, start_us, len_us))?;

            // 4) 边界校验：piece 引用的字节范围必须落在对应 buffer 内
            match source {
                Source::Original => {
                    // 若 original 为 None，则不应出现 Source::Original 的 piece
                    if self.original.is_none() {
                        return Err(format!(
                            "piece {} 引用 Source::Original 但 original buffer 不存在",
                            i
                        ));
                    }
                    if end > original_len {
                        return Err(format!(
                            "piece {} Original 越界: start+len={} > original_len={}",
                            i, end, original_len
                        ));
                    }
                }
                Source::Add => {
                    if end > state.add_buffer_len {
                        return Err(format!(
                            "piece {} Add 越界: start+len={} > add_buffer_len={}",
                            i, end, state.add_buffer_len
                        ));
                    }
                }
            }

            // 5) line_breaks 不应超过 len（每个换行符至少占 1 字节）
            if (lb_us as usize) > len_us {
                return Err(format!(
                    "piece {} line_breaks={} 超过 len={}",
                    i, lb_us, len_us
                ));
            }

            total_bytes = total_bytes
                .checked_add(len)
                .ok_or_else(|| format!("piece {} 累加 len 溢出 u64", i))?;

            pieces.push(Piece {
                source,
                start: start_us,
                len: len_us,
                line_breaks: lb_us,
            });
        }

        // 6) 总字节数交叉校验
        if total_bytes as usize != state.byte_len {
            return Err(format!(
                "byte_len 不匹配: pieces 累加={} state={}",
                total_bytes, state.byte_len
            ));
        }

        self.pieces = pieces;
        self.len_lines = state.line_count;
        self.len_chars = state.byte_len;
        self.rebuild_line_index();
        Ok(())
    }

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
