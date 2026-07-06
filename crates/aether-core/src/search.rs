use std::path::{Path, PathBuf};

use regex::RegexBuilder;
use walkdir::WalkDir;

/// 全局搜索查询
#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub pattern: String,
    /// 是否使用正则表达式
    pub regex: bool,
    /// 是否区分大小写
    pub case_sensitive: bool,
    /// 包含的文件 glob 模式（空表示全部）
    pub include: Vec<String>,
    /// 排除的文件 glob 模式
    pub exclude: Vec<String>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            regex: false,
            case_sensitive: false,
            include: Vec::new(),
            exclude: vec![
                "target".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
            ],
        }
    }
}

/// 搜索结果项
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub text: String,
}

/// 在工作区中执行文本搜索
///
/// 优先使用系统 `rg`（ripgrep）；不可用时回退到 walkdir + regex。
pub fn search_workspace(root_dir: &Path, query: &SearchQuery) -> Vec<SearchResult> {
    if query.pattern.is_empty() {
        return Vec::new();
    }

    // 优先尝试 ripgrep
    if let Some(results) = search_with_ripgrep(root_dir, query) {
        return results;
    }

    search_with_walkdir(root_dir, query)
}

fn search_with_ripgrep(root_dir: &Path, query: &SearchQuery) -> Option<Vec<SearchResult>> {
    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--line-number")
        .arg("--column")
        .arg("--max-count")
        .arg("50")
        .arg("--max-filesize")
        .arg("1M");

    if !query.case_sensitive {
        cmd.arg("--ignore-case");
    }
    if query.regex {
        cmd.arg("--regexp");
    } else {
        cmd.arg("--fixed-strings");
    }
    for inc in &query.include {
        cmd.arg("--glob").arg(inc);
    }
    for exc in &query.exclude {
        cmd.arg("--glob").arg(format!("!{}", exc));
    }

    cmd.arg(&query.pattern).current_dir(root_dir);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Some(parse_ripgrep_output(root_dir, &text))
}

fn parse_ripgrep_output(root_dir: &Path, output: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for line in output.lines() {
        // rg --line-number --column 输出格式: path:line:col:matched text
        let Some((path_part, rest)) = line.split_once(':') else {
            continue;
        };
        let Some((line_part, rest)) = rest.split_once(':') else {
            continue;
        };
        let Some((col_part, text)) = rest.split_once(':') else {
            continue;
        };
        let Ok(line_no) = line_part.parse::<usize>() else {
            continue;
        };
        let Ok(col_no) = col_part.parse::<usize>() else {
            continue;
        };
        results.push(SearchResult {
            path: root_dir.join(path_part),
            line: line_no,
            col: col_no,
            text: text.to_string(),
        });
        if results.len() >= 500 {
            break;
        }
    }
    results
}

fn search_with_walkdir(root_dir: &Path, query: &SearchQuery) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let max_results = 500;

    let regex = match build_regex(query) {
        Ok(r) => r,
        Err(_) => return results,
    };

    for entry in WalkDir::new(root_dir).follow_links(false) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_excluded(path, root_dir, query) {
            continue;
        }
        if !is_included(path, root_dir, query) {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > 1024 * 1024 {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            for (line_idx, line) in content.lines().enumerate() {
                for m in regex.find_iter(line) {
                    results.push(SearchResult {
                        path: path.to_path_buf(),
                        line: line_idx + 1,
                        col: m.start() + 1,
                        text: line.to_string(),
                    });
                    if results.len() >= max_results {
                        return results;
                    }
                }
            }
        }
    }

    results
}

fn build_regex(query: &SearchQuery) -> Result<regex::Regex, regex::Error> {
    let pattern = if query.regex {
        query.pattern.clone()
    } else {
        regex::escape(&query.pattern)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!query.case_sensitive)
        .build()
}

fn is_excluded(path: &Path, root: &Path, query: &SearchQuery) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    for pat in &query.exclude {
        if relative.to_string_lossy().contains(pat) {
            return true;
        }
    }
    false
}

fn is_included(path: &Path, root: &Path, query: &SearchQuery) -> bool {
    if query.include.is_empty() {
        return true;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    let s = relative.to_string_lossy();
    query.include.iter().any(|pat| s.contains(pat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_regex_literal() {
        let q = SearchQuery {
            pattern: "fn main".to_string(),
            regex: false,
            ..Default::default()
        };
        let re = build_regex(&q).unwrap();
        assert!(re.is_match("fn main() {}"));
    }

    #[test]
    fn test_build_regex_case_insensitive() {
        let q = SearchQuery {
            pattern: "HELLO".to_string(),
            regex: false,
            case_sensitive: false,
            ..Default::default()
        };
        let re = build_regex(&q).unwrap();
        assert!(re.is_match("hello world"));
    }

    #[test]
    fn test_search_query_default() {
        let q = SearchQuery::default();
        assert!(q.pattern.is_empty());
        assert!(!q.regex);
        assert!(!q.case_sensitive);
        assert!(q.include.is_empty());
        assert_eq!(q.exclude.len(), 3);
    }

    #[test]
    fn test_build_regex_literal_escapes() {
        let q = SearchQuery {
            pattern: "a.b".to_string(),
            regex: false,
            case_sensitive: true,
            ..Default::default()
        };
        let re = build_regex(&q).unwrap();
        assert!(re.is_match("a.b"));
        assert!(!re.is_match("aXb"));
    }

    #[test]
    fn test_build_regex_regex_mode() {
        let q = SearchQuery {
            pattern: r"\d+".to_string(),
            regex: true,
            case_sensitive: true,
            ..Default::default()
        };
        let re = build_regex(&q).unwrap();
        assert!(re.is_match("abc123"));
        assert!(!re.is_match("abc"));
    }

    #[test]
    fn test_build_regex_invalid() {
        let q = SearchQuery {
            pattern: "(".to_string(),
            regex: true,
            ..Default::default()
        };
        assert!(build_regex(&q).is_err());
    }

    #[test]
    fn test_parse_ripgrep_output() {
        let root = std::path::Path::new("/root");
        let output = "src/main.rs:10:5:fn main() {\nsrc/lib.rs:20:1:pub fn add() {}";
        let results = parse_ripgrep_output(root, output);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, root.join("src/main.rs"));
        assert_eq!(results[0].line, 10);
        assert_eq!(results[0].col, 5);
        assert_eq!(results[1].line, 20);
    }

    #[test]
    fn test_parse_ripgrep_output_malformed() {
        let root = std::path::Path::new("/root");
        let output = "no-colons\n:::\n";
        let results = parse_ripgrep_output(root, output);
        assert!(results.is_empty());
    }

    #[test]
    fn test_is_excluded() {
        let q = SearchQuery {
            exclude: vec!["target".to_string(), ".git".to_string()],
            ..Default::default()
        };
        let root = std::path::Path::new("/proj");
        assert!(is_excluded(std::path::Path::new("/proj/target/debug/foo"), root, &q));
        assert!(is_excluded(std::path::Path::new("/proj/.git/config"), root, &q));
        assert!(!is_excluded(std::path::Path::new("/proj/src/main.rs"), root, &q));
    }

    #[test]
    fn test_is_included() {
        let q = SearchQuery {
            include: vec!["src".to_string()],
            ..Default::default()
        };
        let root = std::path::Path::new("/proj");
        assert!(is_included(std::path::Path::new("/proj/src/main.rs"), root, &q));
        assert!(!is_included(std::path::Path::new("/proj/tests/t.rs"), root, &q));

        let empty = SearchQuery::default();
        assert!(is_included(std::path::Path::new("/proj/any"), root, &empty));
    }
}
