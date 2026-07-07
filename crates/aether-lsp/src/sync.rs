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
    if !old_text.is_empty() && (old_end - old_start) > old_text.len() / 2 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn test_document_sync_open_close() {
        let mut sync = DocumentSync::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        sync.open_document(
            uri.clone(),
            "rust".to_string(),
            1,
            "fn main() {}".to_string(),
        );
        assert!(sync.is_open(&uri));
        assert_eq!(sync.get_language_id(&uri).unwrap(), "rust");
        assert_eq!(sync.get_version(&uri).unwrap(), 1);

        sync.close_document(&uri);
        assert!(!sync.is_open(&uri));
        assert!(sync.get_document(&uri).is_none());
    }

    #[test]
    fn test_document_sync_version_and_text() {
        let mut sync = DocumentSync::new();
        let uri = Url::parse("file:///test.rs").unwrap();
        sync.open_document(uri.clone(), "rust".to_string(), 1, "a".to_string());
        assert_eq!(sync.increment_version(&uri).unwrap(), 2);
        assert_eq!(sync.get_version(&uri).unwrap(), 2);

        sync.update_text(&uri, "b".to_string());
        assert_eq!(sync.get_document(&uri).unwrap().text, "b");

        // 不存在的文档
        let missing = Url::parse("file:///missing.rs").unwrap();
        assert!(sync.increment_version(&missing).is_none());
        assert!(sync.get_version(&missing).is_none());
    }

    #[test]
    fn test_compute_changes_no_change() {
        let changes = compute_changes("hello", "hello");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_changes_insert() {
        let changes = compute_changes("hello", "hello world");
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(
            change.range,
            Some(Range {
                start: pos(0, 5),
                end: pos(0, 5)
            })
        );
        assert_eq!(change.text, " world");
    }

    #[test]
    fn test_compute_changes_delete() {
        // 删除部分不超过原文 50%,应生成增量变更
        let changes = compute_changes("hello beautiful world", "hello world");
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        // 共同前缀 "hello ",共同后缀 "world",删除中间 "beautiful "
        assert_eq!(
            change.range,
            Some(Range {
                start: pos(0, 6),
                end: pos(0, 16)
            })
        );
        assert_eq!(change.text, "");
    }

    #[test]
    fn test_compute_changes_replace() {
        let changes = compute_changes("hello world", "hello rust");
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(
            change.range,
            Some(Range {
                start: pos(0, 6),
                end: pos(0, 11)
            })
        );
        assert_eq!(change.text, "rust");
    }

    #[test]
    fn test_compute_changes_large_file_fallback() {
        let old_text = "a".repeat(100_001);
        let new_text = old_text.clone() + "b";
        let changes = compute_changes(&old_text, &new_text);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].range.is_none());
        assert_eq!(changes[0].text, new_text);
    }

    #[test]
    fn test_compute_changes_major_change_fallback() {
        // 变更超过 50% 时回退为全文替换
        let old_text = "abcdef";
        let new_text = "xyz";
        let changes = compute_changes(old_text, new_text);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].range.is_none());
        assert_eq!(changes[0].text, new_text);
    }

    #[test]
    fn test_compute_changes_multiline() {
        let old_text = "line1\nline2\nline3";
        let new_text = "line1\nmodified\nline3";
        let changes = compute_changes(old_text, new_text);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(
            change.range,
            Some(Range {
                start: pos(1, 0),
                end: pos(1, 5),
            })
        );
        assert_eq!(change.text, "modified");
    }

    #[test]
    fn test_compute_changes_utf16_character_count() {
        // "𐍈" 是一个 UTF-16 代理对,占用 2 个 UTF-16 码元
        let old_text = "a";
        let new_text = "a𐍈";
        let changes = compute_changes(old_text, new_text);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(
            change.range,
            Some(Range {
                start: pos(0, 1),
                end: pos(0, 1)
            })
        );
        assert_eq!(change.text, "𐍈");
    }
}
