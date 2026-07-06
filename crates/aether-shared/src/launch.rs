use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// 传递给 GUI 主程序的启动参数
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LaunchArgs {
    pub paths: Vec<PathBuf>,
    pub new_window: bool,
    pub goto: Option<GotoPosition>,
    pub wait: bool,
}

impl LaunchArgs {
    /// 从命令行解析由 aether-cli 注入的 JSON 参数
    pub fn from_env() -> Self {
        let mut iter = std::env::args();
        let _ = iter.next(); // 跳过可执行文件名称

        while let Some(arg) = iter.next() {
            if arg == "--aether-launch-args" {
                if let Some(json) = iter.next() {
                    if let Ok(parsed) = serde_json::from_str(&json) {
                        return parsed;
                    }
                }
            }
        }

        Self::default()
    }

    /// 是否没有任何启动路径或特殊指令
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.goto.is_none()
    }
}

/// goto 位置参数，支持两种形式：
/// - `file.txt:10:5` / `file.txt:10`
/// - `10:5` / `10`（配合路径参数使用）
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GotoPosition {
    pub line: usize,
    pub column: usize,
}

impl GotoPosition {
    /// 将 1-based 行号转成 0-based 内部索引
    pub fn zero_based_line(&self) -> usize {
        self.line.saturating_sub(1)
    }

    /// 将 1-based 列号转成 0-based 内部索引
    pub fn zero_based_column(&self) -> usize {
        self.column.saturating_sub(1)
    }
}

impl FromStr for GotoPosition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_goto_position(s)
    }
}

impl std::fmt::Display for GotoPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// 解析纯位置字符串（不含文件路径）：10:5 或 10
fn parse_goto_position(s: &str) -> Result<GotoPosition, String> {
    if s.is_empty() {
        return Err("goto 位置不能为空".to_string());
    }

    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err("goto 位置格式错误，应为 line 或 line:column".to_string());
    }

    let line = parts[0]
        .parse::<usize>()
        .map_err(|_| "goto 行号必须是数字".to_string())?;
    if line == 0 {
        return Err("goto 行号必须从 1 开始".to_string());
    }

    let column = if parts.len() == 2 {
        parts[1]
            .parse::<usize>()
            .map_err(|_| "goto 列号必须是数字".to_string())?
    } else {
        1
    };

    Ok(GotoPosition { line, column })
}

/// 解析完整 goto 字符串，返回（可选的文件路径，位置）。
///
/// 支持：
/// - `file.txt:10:5` / `file.txt:10`
/// - `10:5` / `10`（此时文件为 None）
/// - Windows 绝对路径：`C:\folder\file.txt:10:5`
pub fn parse_goto(s: &str) -> Result<(Option<PathBuf>, GotoPosition), String> {
    if s.is_empty() {
        return Err("goto 不能为空".to_string());
    }

    // 从右侧开始找行号/列号：最后一段和倒数第二段（如果都是数字）
    let mut parts: Vec<&str> = s.rsplit(':').collect();
    if parts.len() < 2 {
        // 可能是纯位置如 "10"
        return Ok((None, parse_goto_position(s)?));
    }

    // 尝试最后一段作为列号
    let maybe_col = parts[0].parse::<usize>();
    parts.remove(0);

    // 尝试倒数第二段作为行号
    let maybe_line = parts[0].parse::<usize>();

    let (file_str, line, column) = match (maybe_line, maybe_col) {
        (Ok(line), Ok(col)) if line > 0 => {
            parts.remove(0); // line
            let file = parts.iter().rev().cloned().collect::<Vec<_>>().join(":");
            (file, line, col)
        }
        (Err(_), Ok(line)) if line > 0 => {
            // 只有行号，列号默认 1
            // 此时 "倒数第二段" 其实是文件的一部分，需要把它放回去
            let file = parts.iter().rev().cloned().collect::<Vec<_>>().join(":");
            (file, line, 1)
        }
        _ => {
            // 没有数字后缀，尝试整体作为纯位置
            return Ok((None, parse_goto_position(s)?));
        }
    };

    let file = if file_str.is_empty() {
        None
    } else {
        Some(PathBuf::from(file_str))
    };

    Ok((file, GotoPosition { line, column }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_goto_position() {
        assert_eq!(
            parse_goto_position("10:5").unwrap(),
            GotoPosition { line: 10, column: 5 }
        );
        assert_eq!(
            parse_goto_position("10").unwrap(),
            GotoPosition { line: 10, column: 1 }
        );
    }

    #[test]
    fn test_parse_goto_with_file() {
        let (file, pos) = parse_goto("file.txt:10:5").unwrap();
        assert_eq!(file, Some(PathBuf::from("file.txt")));
        assert_eq!(pos, GotoPosition { line: 10, column: 5 });

        let (file, pos) = parse_goto("file.txt:10").unwrap();
        assert_eq!(file, Some(PathBuf::from("file.txt")));
        assert_eq!(pos, GotoPosition { line: 10, column: 1 });

        let (file, pos) = parse_goto("C:\\folder\\file.txt:10:5").unwrap();
        assert_eq!(file, Some(PathBuf::from("C:\\folder\\file.txt")));
        assert_eq!(pos, GotoPosition { line: 10, column: 5 });

        let (file, pos) = parse_goto("src:main.rs:10:5").unwrap();
        assert_eq!(file, Some(PathBuf::from("src:main.rs")));
        assert_eq!(pos, GotoPosition { line: 10, column: 5 });
    }

    #[test]
    fn test_parse_goto_without_file() {
        let (file, pos) = parse_goto("10:5").unwrap();
        assert_eq!(file, None);
        assert_eq!(pos, GotoPosition { line: 10, column: 5 });

        let (file, pos) = parse_goto("10").unwrap();
        assert_eq!(file, None);
        assert_eq!(pos, GotoPosition { line: 10, column: 1 });
    }

    #[test]
    fn test_parse_goto_errors() {
        assert!(parse_goto("").is_err());
        assert!(parse_goto("file.txt").is_err());
        assert!(parse_goto("file.txt:abc").is_err());
        assert!(parse_goto("file.txt:0:5").is_err());
    }

    #[test]
    fn test_launch_args_default_and_is_empty() {
        let args = LaunchArgs::default();
        assert!(args.is_empty(), "默认 LaunchArgs 应为空");
        assert!(args.paths.is_empty());
        assert!(args.goto.is_none());
        assert!(!args.new_window);
        assert!(!args.wait);
    }

    #[test]
    fn test_launch_args_is_empty_with_paths_or_goto() {
        let with_path = LaunchArgs {
            paths: vec![PathBuf::from("/tmp")],
            ..Default::default()
        };
        assert!(!with_path.is_empty());

        let with_goto = LaunchArgs {
            goto: Some(GotoPosition { line: 1, column: 1 }),
            ..Default::default()
        };
        assert!(!with_goto.is_empty());
    }

    #[test]
    fn test_launch_args_roundtrip_json() {
        let args = LaunchArgs {
            paths: vec![PathBuf::from("C:\\src\\main.rs")],
            new_window: true,
            goto: Some(GotoPosition { line: 10, column: 5 }),
            wait: false,
        };
        let json = serde_json::to_string(&args).expect("序列化失败");
        let back: LaunchArgs = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(back.paths, args.paths);
        assert_eq!(back.goto, args.goto);
        assert_eq!(back.new_window, args.new_window);
        assert_eq!(back.wait, args.wait);
    }

    #[test]
    fn test_goto_position_zero_based() {
        let pos = GotoPosition { line: 10, column: 5 };
        assert_eq!(pos.zero_based_line(), 9);
        assert_eq!(pos.zero_based_column(), 4);
        // line=1 / column=1 应映射到 0
        let first = GotoPosition { line: 1, column: 1 };
        assert_eq!(first.zero_based_line(), 0);
        assert_eq!(first.zero_based_column(), 0);
    }

    #[test]
    fn test_goto_position_display_and_from_str() {
        let pos = GotoPosition { line: 7, column: 3 };
        assert_eq!(format!("{}", pos), "7:3");
        let parsed: GotoPosition = "7:3".parse().expect("应解析成功");
        assert_eq!(parsed, pos);
    }

    #[test]
    fn test_parse_goto_position_error_branches() {
        // 空字符串
        assert!(parse_goto_position("").is_err());
        // 行号为 0
        assert!(parse_goto_position("0:5").is_err());
        // 非数字行号
        assert!(parse_goto_position("abc:5").is_err());
        // 非数字列号
        assert!(parse_goto_position("10:abc").is_err());
        // 段数过多
        assert!(parse_goto_position("10:5:7").is_err());
    }

    #[test]
    fn test_parse_goto_with_col_zero() {
        // 在 parse_goto 中，line > 0 且 col 可以为 0
        let (file, pos) = parse_goto("file.txt:10:0").unwrap();
        assert_eq!(file, Some(PathBuf::from("file.txt")));
        assert_eq!(pos, GotoPosition { line: 10, column: 0 });
    }

    #[test]
    fn test_parse_goto_file_only_is_error() {
        // 只有文件名没有行号/列号
        assert!(parse_goto("file.txt").is_err());
    }

    #[test]
    fn test_parse_goto_with_extra_colons_falls_back() {
        // 无法从右侧识别出行号列号，应作为整体位置解析并失败
        assert!(parse_goto("a:b:c:d").is_err());
    }

    #[test]
    fn test_goto_position_zero_based_saturating() {
        // 直接构造 line=0 时，saturating_sub 应保持 0
        let pos = GotoPosition { line: 0, column: 0 };
        assert_eq!(pos.zero_based_line(), 0);
        assert_eq!(pos.zero_based_column(), 0);
    }

    #[test]
    fn test_goto_position_from_str_error() {
        assert!("abc".parse::<GotoPosition>().is_err());
        assert!("0:1".parse::<GotoPosition>().is_err());
        assert!("1:2:3".parse::<GotoPosition>().is_err());
    }
}
