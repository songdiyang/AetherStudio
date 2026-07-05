use lsp_types::*;
use std::collections::HashMap;

use crate::incremental_sync::{FastLineIndex, LineIndexProvider};

/// 文档同步管理器
/// 跟踪所有已打开文档的状态和版本
pub struct DocumentSync {
    documents: HashMap<Url, DocumentState>,
}

impl Default for DocumentSync {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentSync {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open_document(&mut self, uri: Url, language_id: String, version: i32, text: String) {
        self.documents.insert(
            uri.clone(),
            DocumentState {
                uri,
                language_id,
                version,
                text,
            },
        );
    }

    pub fn close_document(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get_document(&self, uri: &Url) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    pub fn get_language_id(&self, uri: &Url) -> Option<&String> {
        self.documents.get(uri).map(|d| &d.language_id)
    }

    pub fn increment_version(&mut self, uri: &Url) -> Option<i32> {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.version += 1;
            Some(doc.version)
        } else {
            None
        }
    }

    pub fn get_version(&self, uri: &Url) -> Option<i32> {
        self.documents.get(uri).map(|d| d.version)
    }

    pub fn update_text(&mut self, uri: &Url, text: String) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.text = text;
        }
    }

    pub fn is_open(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }
}

/// 文档状态
#[derive(Clone, Debug)]
pub struct DocumentState {
    pub uri: Url,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

/// 计算增量变更（基于共同前缀/后缀的字符级 diff）
///
/// 相比原行级启发式，本实现：
/// - 精确到字节级别的变更范围，不再按整行替换
/// - 使用 FastLineIndex 将字节偏移转换为 LSP Position，且 character 按 UTF-16 码元计数
/// - 大文件或变更范围过大时回退为全文替换
pub fn compute_changes(old_text: &str, new_text: &str) -> Vec<TextDocumentContentChangeEvent> {
    // 如果文本差异很大，直接发送完整内容更高效
    const LARGE_FILE_THRESHOLD: usize = 100_000; // 100KB
    if old_text.len() > LARGE_FILE_THRESHOLD || new_text.len() > LARGE_FILE_THRESHOLD {
        return vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new_text.to_string(),
        }];
    }

    // 找到共同前缀长度
    let prefix_len = old_text
        .bytes()
        .zip(new_text.bytes())
        .take_while(|(a, b)| a == b)
        .count();

    // 找到共同后缀长度（不得超过剩余长度）
    let suffix_len = old_text[prefix_len..]
        .bytes()
        .rev()
        .zip(new_text[prefix_len..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let old_start = prefix_len;
    let old_end = old_text.len() - suffix_len;
    let new_start = prefix_len;
    let new_end = new_text.len() - suffix_len;

    // 没有变化
    if old_start == old_end && new_start == new_end {
        return vec![];
    }

    // 如果变更范围超过原文本50%，发送全文替换
    if old_text.len() > 0 && (old_end - old_start) > old_text.len() / 2 {
        return vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new_text.to_string(),
        }];
    }

    // 使用 FastLineIndex 将字节偏移转换为 LSP Position（UTF-16 码元）
    let old_index = FastLineIndex::from_text(old_text);
    let start_pos = old_index.byte_to_position(old_start);
    let end_pos = old_index.byte_to_position(old_end);

    let replacement = new_text[new_start..new_end].to_string();

    vec![TextDocumentContentChangeEvent {
        range: Some(Range {
            start: start_pos,
            end: end_pos,
        }),
        range_length: Some((old_end - old_start) as u32),
        text: replacement,
    }]
}
