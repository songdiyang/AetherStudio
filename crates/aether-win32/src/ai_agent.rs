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

/// 从 AI 回复中解析待执行的终端命令
///
/// 支持标记：
/// ```text
/// <<<<<<< RUN >>>>>>>
/// python src/main.py
/// >>>>>>> END RUN >>>>>>>
/// ```
/// 每个 RUN 块内可包含一条或多条命令（按行拆分，空行忽略）。
pub fn parse_run_commands(response: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut remaining = response;

    while let Some(start) = remaining.find("<<<<<<< RUN") {
        remaining = &remaining[start..];
        // 跳过起始标记到行尾
        let Some(header_end) = remaining.find(">>>>>>>") else {
            break;
        };
        let after_header = &remaining[header_end + ">>>>>>>".len()..];

        // 查找结束标记
        let Some(end_start) = after_header.find(">>>>>>> END RUN") else {
            break;
        };
        let body = &after_header[..end_start];
        for line in body.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                commands.push(trimmed.to_string());
            }
        }
        // 跳过结束标记到行尾
        let after_end = &after_header[end_start..];
        let end_marker_end = after_end
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or(after_end.len());
        remaining = &after_end[end_marker_end..];
    }

    commands
}

/// 文件操作类型（用于面板清晰展示 AI 执行了什么）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOpKind {
    Create,
    Modify,
    Delete,
}

/// 面板展示用的有序块：普通文本 / 文件操作 / 运行命令。
///
/// 目的：把 AI 回复里的 `<<<<<<< FILE/RUN >>>>>>>` 原始标记转成清晰的操作提示，
/// 让用户直观看到"新建 / 修改 / 删除了哪个文件、运行了什么命令"，而不是一堆尖括号标记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentDisplayBlock {
    Text(String),
    File { kind: FileOpKind, path: String },
    Run { cmd: String },
}

fn push_text_block(blocks: &mut Vec<AgentDisplayBlock>, s: &str) {
    let t = s.trim_matches('\n');
    if !t.trim().is_empty() {
        blocks.push(AgentDisplayBlock::Text(t.to_string()));
    }
}

/// 将 AI 回复按出现顺序解析为"文本 + 操作"块，隐藏原始标记，供面板渲染操作卡片。
///
/// 解析失败/标记不完整时，剩余内容作为普通文本返回（不丢内容）。
pub fn parse_display_blocks(response: &str) -> Vec<AgentDisplayBlock> {
    let mut blocks: Vec<AgentDisplayBlock> = Vec::new();
    let mut rest = response;
    loop {
        let file_pos = rest.find("<<<<<<< FILE");
        let run_pos = rest.find("<<<<<<< RUN");
        let (pos, is_file) = match (file_pos, run_pos) {
            (None, None) => {
                push_text_block(&mut blocks, rest);
                break;
            }
            (Some(f), None) => (f, true),
            (None, Some(r)) => (r, false),
            (Some(f), Some(r)) => {
                if f < r {
                    (f, true)
                } else {
                    (r, false)
                }
            }
        };
        push_text_block(&mut blocks, &rest[..pos]);
        let after = &rest[pos..];

        if is_file {
            let Some(hend) = after.find(">>>>>>>") else {
                push_text_block(&mut blocks, after);
                break;
            };
            let header = &after[..hend + ">>>>>>>".len()];
            let path = extract_path_from_header(header).unwrap_or_else(|| "unknown".to_string());
            let body_rest = &after[hend + ">>>>>>>".len()..];
            let Some(sep) = body_rest.find("=======") else {
                push_text_block(&mut blocks, after);
                break;
            };
            let search = &body_rest[..sep];
            let after_sep = &body_rest[sep + "=======".len()..];
            let Some(eend) = after_sep.find(">>>>>>> END FILE") else {
                push_text_block(&mut blocks, after);
                break;
            };
            let replace = &after_sep[..eend];
            let kind = if search.trim().is_empty() {
                FileOpKind::Create
            } else if replace.trim().is_empty() {
                FileOpKind::Delete
            } else {
                FileOpKind::Modify
            };
            blocks.push(AgentDisplayBlock::File {
                kind,
                path: path.trim().to_string(),
            });
            let after_end = &after_sep[eend..];
            let skip = after_end
                .find('\n')
                .map(|i| i + 1)
                .unwrap_or(after_end.len());
            rest = &after_end[skip..];
        } else {
            let Some(hend) = after.find(">>>>>>>") else {
                push_text_block(&mut blocks, after);
                break;
            };
            let after_header = &after[hend + ">>>>>>>".len()..];
            let Some(eend) = after_header.find(">>>>>>> END RUN") else {
                push_text_block(&mut blocks, after);
                break;
            };
            let body = &after_header[..eend];
            for line in body.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    blocks.push(AgentDisplayBlock::Run { cmd: t.to_string() });
                }
            }
            let after_end = &after_header[eend..];
            let skip = after_end
                .find('\n')
                .map(|i| i + 1)
                .unwrap_or(after_end.len());
            rest = &after_end[skip..];
        }
    }
    blocks
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

    #[test]
    fn test_parse_run_commands_single() {
        let text = r#"我将运行脚本：
<<<<<<< RUN >>>>>>>
python src/main.py
>>>>>>> END RUN >>>>>>>
"#;
        let cmds = parse_run_commands(text);
        assert_eq!(cmds, vec!["python src/main.py".to_string()]);
    }

    #[test]
    fn test_parse_run_commands_multi() {
        let text = r#"<<<<<<< RUN >>>>>>>
cargo build
cargo test
>>>>>>> END RUN >>>>>>>"#;
        let cmds = parse_run_commands(text);
        assert_eq!(
            cmds,
            vec!["cargo build".to_string(), "cargo test".to_string()]
        );
    }

    #[test]
    fn test_parse_run_commands_none() {
        let cmds = parse_run_commands("没有命令");
        assert!(cmds.is_empty());
    }
}
