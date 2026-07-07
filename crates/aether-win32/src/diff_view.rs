use std::path::{Path, PathBuf};

use similar::{ChangeTag, TextDiff};

use crate::ai_agent::{parse_edits, AiEdit};

fn resolve_edit_path(path: &Path, current_folder: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    current_folder
        .map(|root| root.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Diff 行类型
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    /// 未变更的上下文行
    Context,
    /// 旧文本中被删除的行
    Delete,
    /// 新文本中新增的行
    Insert,
}

/// 单条 diff 行
#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_line_no: Option<usize>,
    pub new_line_no: Option<usize>,
}

/// 单个文件的 diff 预览
#[derive(Clone, Debug)]
pub struct DiffFile {
    pub path: PathBuf,
    pub original: String,
    pub proposed: String,
    pub lines: Vec<DiffLine>,
    pub accepted: bool,
    pub rejected: bool,
}

impl DiffFile {
    pub fn new(path: PathBuf, original: String, proposed: String) -> Self {
        let lines = build_diff_lines(&original, &proposed);
        Self {
            path,
            original,
            proposed,
            lines,
            accepted: false,
            rejected: false,
        }
    }

    /// 新增/删除行总数，用于 UI 摘要
    pub fn change_count(&self) -> (usize, usize) {
        let mut del = 0;
        let mut ins = 0;
        for line in &self.lines {
            match line.kind {
                DiffLineKind::Delete => del += 1,
                DiffLineKind::Insert => ins += 1,
                _ => {}
            }
        }
        (del, ins)
    }
}

/// AI 编辑的 diff 预览面板
#[derive(Clone, Debug, Default)]
pub struct DiffView {
    pub files: Vec<DiffFile>,
    pub selected_index: usize,
}

impl DiffView {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected_index: 0,
        }
    }

    /// 从 AI 回复文本生成 diff 视图
    pub fn from_response(
        response: &str,
        default_path: Option<&Path>,
        current_folder: Option<&Path>,
    ) -> Self {
        let default = default_path.map(|p| p.to_string_lossy().to_string());
        let edits = parse_edits(response, default.as_deref());
        Self::from_edits(&edits, current_folder)
    }

    /// 从已解析的编辑列表生成 diff 视图
    pub fn from_edits(edits: &[AiEdit], current_folder: Option<&Path>) -> Self {
        let mut files = Vec::new();
        for edit in edits {
            let full_path = resolve_edit_path(&edit.path, current_folder);
            let original = if full_path.exists() {
                std::fs::read_to_string(&full_path).unwrap_or_default()
            } else {
                String::new()
            };
            let proposed = if edit.search.trim().is_empty() {
                edit.replace.clone()
            } else {
                match original.find(&edit.search) {
                    Some(pos) => {
                        let mut replaced = original.clone();
                        replaced.replace_range(pos..pos + edit.search.len(), &edit.replace);
                        replaced
                    }
                    None => original.clone(),
                }
            };
            files.push(DiffFile::new(full_path, original, proposed));
        }
        Self {
            files,
            selected_index: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn selected_file(&self) -> Option<&DiffFile> {
        self.files.get(self.selected_index)
    }

    pub fn selected_file_mut(&mut self) -> Option<&mut DiffFile> {
        self.files.get_mut(self.selected_index)
    }

    pub fn next_file(&mut self) {
        if !self.files.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.files.len();
        }
    }

    pub fn prev_file(&mut self) {
        if !self.files.is_empty() {
            self.selected_index = (self.selected_index + self.files.len() - 1) % self.files.len();
        }
    }

    /// 接受所有未拒绝的文件
    pub fn accept_all(&mut self) {
        for f in &mut self.files {
            if !f.rejected {
                f.accepted = true;
            }
        }
    }

    /// 拒绝所有未接受的文件
    pub fn reject_all(&mut self) {
        for f in &mut self.files {
            if !f.accepted {
                f.rejected = true;
            }
        }
    }

    /// 获取所有已接受的文件
    pub fn accepted_files(&self) -> Vec<&DiffFile> {
        self.files.iter().filter(|f| f.accepted).collect()
    }

    /// 生成可被 EditorState 应用的实际 AiEdit 列表
    pub fn to_edits(&self) -> Vec<AiEdit> {
        self.files
            .iter()
            .filter(|f| f.accepted)
            .map(|f| AiEdit {
                path: f.path.clone(),
                search: f.original.clone(),
                replace: f.proposed.clone(),
            })
            .collect()
    }
}

fn build_diff_lines(old_text: &str, new_text: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old_text, new_text);
    let mut lines = Vec::new();
    let mut old_line = 1usize;
    let mut new_line = 1usize;

    for change in diff.iter_all_changes() {
        let text = change.value().to_string();
        let kind = match change.tag() {
            ChangeTag::Equal => DiffLineKind::Context,
            ChangeTag::Delete => DiffLineKind::Delete,
            ChangeTag::Insert => DiffLineKind::Insert,
        };
        let (old_no, new_no) = match kind {
            DiffLineKind::Context => {
                let o = old_line;
                let n = new_line;
                old_line += 1;
                new_line += 1;
                (Some(o), Some(n))
            }
            DiffLineKind::Delete => {
                let o = old_line;
                old_line += 1;
                (Some(o), None)
            }
            DiffLineKind::Insert => {
                let n = new_line;
                new_line += 1;
                (None, Some(n))
            }
        };
        lines.push(DiffLine {
            kind,
            text,
            old_line_no: old_no,
            new_line_no: new_no,
        });
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_edit_path() {
        let abs = PathBuf::from("D:\\workspace\\file.rs");
        assert_eq!(resolve_edit_path(&abs, None), abs);

        let rel = PathBuf::from("src/main.rs");
        let root = Path::new("D:\\workspace");
        assert_eq!(
            resolve_edit_path(&rel, Some(root)),
            PathBuf::from("D:\\workspace\\src/main.rs")
        );

        assert_eq!(resolve_edit_path(&rel, None), rel);
    }

    #[test]
    fn test_build_diff_lines_empty_and_identical() {
        let lines = build_diff_lines("", "hello\n");
        assert!(lines.iter().any(|l| matches!(l.kind, DiffLineKind::Insert)));

        let lines = build_diff_lines("same\n", "same\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!(lines[0].old_line_no, Some(1));
        assert_eq!(lines[0].new_line_no, Some(1));

        let lines = build_diff_lines("", "");
        assert!(lines.is_empty());
    }

    #[test]
    fn test_build_diff_lines_line_numbers() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nchanged\nline3\n";
        let lines = build_diff_lines(old, new);
        let delete = lines
            .iter()
            .find(|l| l.kind == DiffLineKind::Delete)
            .unwrap();
        let insert = lines
            .iter()
            .find(|l| l.kind == DiffLineKind::Insert)
            .unwrap();
        assert_eq!(delete.old_line_no, Some(2));
        assert_eq!(delete.new_line_no, None);
        assert_eq!(insert.old_line_no, None);
        assert_eq!(insert.new_line_no, Some(2));
    }

    #[test]
    fn test_diff_lines() {
        let old = "fn main() {\n    println!(\"hello\");\n}\n";
        let new = "fn main() {\n    println!(\"world\");\n}\n";
        let lines = build_diff_lines(old, new);
        assert!(lines.iter().any(|l| matches!(l.kind, DiffLineKind::Delete)));
        assert!(lines.iter().any(|l| matches!(l.kind, DiffLineKind::Insert)));
    }

    #[test]
    fn test_diff_file_change_count() {
        let df = DiffFile::new(
            PathBuf::from("x.rs"),
            "a\nb\n".to_string(),
            "a\nc\n".to_string(),
        );
        assert_eq!(df.change_count(), (1, 1));

        let empty = DiffFile::new(PathBuf::from("empty.rs"), String::new(), String::new());
        assert_eq!(empty.change_count(), (0, 0));
    }

    #[test]
    fn test_diff_view_empty() {
        let view = DiffView::new();
        assert!(view.is_empty());
        assert!(view.selected_file().is_none());
        assert!(view.to_edits().is_empty());
    }

    #[test]
    fn test_diff_view_from_edits_creates_files() {
        let dir = std::env::temp_dir().join(format!("aether_diff_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("existing.txt");
        std::fs::write(&file_path, "old content\n").unwrap();

        let edits = vec![
            AiEdit::new(
                file_path.clone(),
                "old content\n".to_string(),
                "new content\n".to_string(),
            ),
            AiEdit::new(dir.join("new.txt"), String::new(), "created\n".to_string()),
        ];
        let view = DiffView::from_edits(&edits, Some(&dir));
        assert_eq!(view.files.len(), 2);
        assert_eq!(view.files[0].original, "old content\n");
        assert_eq!(view.files[0].proposed, "new content\n");
        assert_eq!(view.files[1].original, "");
        assert_eq!(view.files[1].proposed, "created\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_diff_view_navigation() {
        let mut view = DiffView::from_edits(
            &[
                AiEdit::new(PathBuf::from("a.txt"), String::new(), "a\n".to_string()),
                AiEdit::new(PathBuf::from("b.txt"), String::new(), "b\n".to_string()),
                AiEdit::new(PathBuf::from("c.txt"), String::new(), "c\n".to_string()),
            ],
            None,
        );
        assert_eq!(view.selected_index, 0);
        view.next_file();
        assert_eq!(view.selected_index, 1);
        view.next_file();
        view.next_file();
        assert_eq!(view.selected_index, 0);
        view.prev_file();
        assert_eq!(view.selected_index, 2);
    }

    #[test]
    fn test_diff_view_accept_reject_and_to_edits() {
        let mut view = DiffView::from_edits(
            &[
                AiEdit::new(PathBuf::from("a.txt"), "old".to_string(), "new".to_string()),
                AiEdit::new(PathBuf::from("b.txt"), "old".to_string(), "new".to_string()),
            ],
            None,
        );

        view.accept_all();
        assert!(view.files[0].accepted);
        assert!(view.files[1].accepted);
        assert_eq!(view.accepted_files().len(), 2);
        let edits = view.to_edits();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path, PathBuf::from("a.txt"));

        // 全部已接受时，reject_all 不会拒绝任何文件
        view.reject_all();
        assert!(!view.files[0].rejected);
        assert!(!view.files[1].rejected);
        assert_eq!(view.accepted_files().len(), 2);

        // 重置状态：先全部拒绝，再接受其中一个
        view.files[0].accepted = false;
        view.files[0].rejected = false;
        view.files[1].accepted = false;
        view.files[1].rejected = false;
        view.reject_all();
        assert!(view.files[0].rejected);
        assert!(view.files[1].rejected);
        assert!(view.accepted_files().is_empty());

        view.files[0].accepted = true;
        view.files[0].rejected = false;
        // 此时 accept_all 只接受未拒绝的 a.txt（已是 accepted），b.txt 已被拒绝故跳过
        view.accept_all();
        assert!(view.files[0].accepted);
        assert!(view.files[1].rejected);
        assert_eq!(view.to_edits().len(), 1);
    }

    #[test]
    fn test_diff_view_search_not_found_leaves_original() {
        let edit = AiEdit::new(
            PathBuf::from("x.txt"),
            "not present".to_string(),
            "replacement".to_string(),
        );
        let view = DiffView::from_edits(&[edit], None);
        assert_eq!(view.files[0].original, view.files[0].proposed);
        assert_eq!(view.files[0].original, "");
    }

    #[test]
    fn test_diff_view_selected_file_mut() {
        let mut view = DiffView::from_edits(
            &[AiEdit::new(
                PathBuf::from("a.txt"),
                String::new(),
                "x".to_string(),
            )],
            None,
        );
        {
            let file = view.selected_file_mut().unwrap();
            file.accepted = true;
        }
        assert!(view.files[0].accepted);
    }
}
