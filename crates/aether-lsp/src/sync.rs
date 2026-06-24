use lsp_types::*;
use std::collections::HashMap;

/// 文档同步管理器
/// 跟踪所有已打开文档的状态和版本
pub struct DocumentSync {
    documents: HashMap<Url, DocumentState>,
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

/// 计算增量变更（简化版：整行替换）
/// 实际生产环境应使用差异算法（如 Myers diff）
pub fn compute_changes(old_text: &str, new_text: &str) -> Vec<TextDocumentContentChangeEvent> {
    // 如果文本差异很大，直接发送完整内容
    if old_text.len() > 10000 || new_text.len() > 10000 {
        return vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new_text.to_string(),
        }];
    }

    // 简单启发式：如果文本差异小，尝试找到变更范围
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    // 找到第一个不同的行
    let mut first_diff = 0;
    while first_diff < old_lines.len()
        && first_diff < new_lines.len()
        && old_lines[first_diff] == new_lines[first_diff]
    {
        first_diff += 1;
    }

    // 找到最后一个不同的行
    let mut old_last = old_lines.len();
    let mut new_last = new_lines.len();
    while old_last > first_diff
        && new_last > first_diff
        && old_lines[old_last - 1] == new_lines[new_last - 1]
    {
        old_last -= 1;
        new_last -= 1;
    }

    if first_diff == old_lines.len() && first_diff == new_lines.len() {
        // 没有变化
        return vec![];
    }

    // 构建变更范围
    let start_line = first_diff;
    let start_char = 0;
    let end_line = old_last.saturating_sub(1);
    let end_char = if end_line < old_lines.len() {
        old_lines[end_line].len()
    } else {
        0
    };

    let replacement: String = if new_last > first_diff {
        new_lines[first_diff..new_last].join("\n")
    } else {
        String::new()
    };

    vec![TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: start_line as u32,
                character: start_char as u32,
            },
            end: Position {
                line: end_line as u32,
                character: end_char as u32,
            },
        }),
        range_length: None,
        text: replacement,
    }]
}
