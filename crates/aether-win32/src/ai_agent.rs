use std::path::PathBuf;

/// AI 建议的单个文件编辑
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiEdit {
    pub path: PathBuf,
    pub search: String,
    pub replace: String,
}

impl AiEdit {
    pub fn new(path: PathBuf, search: String, replace: String) -> Self {
        Self {
            path,
            search,
            replace,
        }
    }

    pub fn is_create_new(&self) -> bool {
        self.search.trim().is_empty()
    }

    pub fn is_delete(&self) -> bool {
        self.replace.trim().is_empty() && !self.search.trim().is_empty()
    }
}

/// 从 AI 回复中解析编辑块
///
/// 支持标记：
/// ```text
/// <<<<<<< FILE src/main.rs >>>>>>>
/// ...old...
/// =======
/// ...new...
/// >>>>>>> END FILE src/main.rs >>>>>>>
/// ```
pub fn parse_edits(response: &str, default_path: Option<&str>) -> Vec<AiEdit> {
    let mut edits = Vec::new();
    let mut remaining = response;

    while let Some(start) = remaining.find("<<<<<<< FILE") {
        remaining = &remaining[start..];

        // 提取路径
        let Some(path_end) = remaining.find(">>>>>>>") else {
            break;
        };
        let header = &remaining[..path_end + ">>>>>>>".len()];
        let path_str = extract_path_from_header(header)
            .unwrap_or_else(|| default_path.unwrap_or("unknown").to_string());
        remaining = &remaining[path_end + ">>>>>>>".len()..];

        // 查找分隔符 =======
        let Some(sep) = remaining.find("=======") else {
            break;
        };
        let search = &remaining[..sep];
        remaining = &remaining[sep + "=======".len()..];

        // 查找结束标记
        let Some(end_marker_start) = remaining.find(">>>>>>> END FILE") else {
            break;
        };
        let replace = &remaining[..end_marker_start];
        // 跳过结束标记到行尾
        let after_end = &remaining[end_marker_start..];
        let end_marker_end = after_end
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or(after_end.len());
        remaining = &after_end[end_marker_end..];

        edits.push(AiEdit::new(
            PathBuf::from(path_str.trim()),
            search.to_string(),
            replace.to_string(),
        ));
    }

    edits
}

fn extract_path_from_header(header: &str) -> Option<String> {
    // header looks like "<<<<<<< FILE src/main.rs >>>>>>>"
    let inner = header
        .strip_prefix("<<<<<<< FILE")?
        .strip_suffix(">>>>>>>")?
        .trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_edit() {
        let text = r#"下面修改 main.rs：
<<<<<<< FILE src/main.rs >>>>>>>
fn old() {}
=======
fn new() {}
>>>>>>> END FILE src/main.rs >>>>>>>
"#;
        let edits = parse_edits(text, None);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path, PathBuf::from("src/main.rs"));
        assert!(edits[0].search.contains("fn old()"));
        assert!(edits[0].replace.contains("fn new()"));
    }

    #[test]
    fn test_parse_create_new_file() {
        let text = r#"<<<<<<< FILE src/lib.rs >>>>>>>
=======
pub fn hello() {}
>>>>>>> END FILE src/lib.rs >>>>>>>
"#;
        let edits = parse_edits(text, None);
        assert_eq!(edits.len(), 1);
        assert!(edits[0].is_create_new());
    }

    #[test]
    fn test_parse_no_markers() {
        let edits = parse_edits("普通回答，没有编辑", None);
        assert!(edits.is_empty());
    }
}
