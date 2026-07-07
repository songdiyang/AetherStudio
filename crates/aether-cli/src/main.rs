use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::Parser;

use aether_shared::launch::{parse_goto, GotoPosition, LaunchArgs};

#[derive(Parser, Debug)]
#[command(name = "aether", about = "Aether Editor 命令行接口", version)]
struct Cli {
    /// 要打开的文件或文件夹路径
    paths: Vec<PathBuf>,

    /// 强制打开新窗口，而不是复用已有窗口
    #[arg(long)]
    new_window: bool,

    /// 等待编辑器关闭后再返回
    #[arg(long)]
    wait: bool,

    /// 打开文件并定位，例如 file.txt:10:5 或 10:5
    #[arg(long)]
    goto: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("aether: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let args = build_launch_args(&cli)?;

    let json = serde_json::to_string(&args).context("序列化启动参数失败")?;
    let app_exe = find_app_exe()?;

    let mut cmd = Command::new(&app_exe);
    cmd.arg("--aether-launch-args").arg(&json);

    if cli.wait {
        let status = cmd.status().context("无法启动 GUI 主程序")?;
        std::process::exit(status.code().unwrap_or(0));
    } else {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动 GUI 主程序")?;
    }

    Ok(())
}

/// 根据 CLI 参数构造启动参数（可独立测试 run 的解析与路径处理逻辑）
fn build_launch_args(cli: &Cli) -> Result<LaunchArgs> {
    let (goto_file, goto_position) = parse_goto_arg(cli.goto.clone())?;

    let mut paths = normalize_paths(cli.paths.clone())?;
    if let Some(file) = goto_file {
        // 如果 goto 中指定了文件，把它放到路径列表最前面，确保优先加载并跳转
        if let Some(pos) = paths.iter().position(|p| p == &file) {
            paths.remove(pos);
        }
        paths.insert(0, file);
    }

    Ok(LaunchArgs {
        paths,
        new_window: cli.new_window,
        goto: goto_position,
        wait: cli.wait,
    })
}

/// 解析 --goto 参数，返回（可选的文件路径，位置）
fn parse_goto_arg(goto: Option<String>) -> Result<(Option<PathBuf>, Option<GotoPosition>)> {
    let Some(s) = goto else {
        return Ok((None, None));
    };

    let (file, position) = parse_goto(&s).map_err(|e| anyhow::anyhow!("--goto 参数错误: {}", e))?;
    Ok((file, Some(position)))
}

/// 规范化路径：相对路径转成绝对路径
fn normalize_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let cwd = env::current_dir().context("无法获取当前工作目录")?;
    paths
        .into_iter()
        .map(|p| {
            if p.is_absolute() {
                Ok(p)
            } else {
                let abs = cwd.join(&p);
                Ok(abs.canonicalize().unwrap_or(abs))
            }
        })
        .collect()
}

/// 查找与 CLI 同目录下的 GUI 主程序
fn find_app_exe() -> Result<PathBuf> {
    let current_exe = env::current_exe().context("无法获取当前可执行文件路径")?;
    find_app_exe_in(&current_exe)
}

/// 在 `current_exe` 所在目录查找 GUI 主程序（抽出以便注入测试）
fn find_app_exe_in(current_exe: &Path) -> Result<PathBuf> {
    let dir = current_exe.parent().context("无法获取可执行文件所在目录")?;

    let app_name = if cfg!(windows) {
        "aether-app.exe"
    } else {
        "aether-app"
    };
    let app_exe = dir.join(app_name);

    if app_exe.exists() {
        Ok(app_exe)
    } else {
        bail!(
            "找不到 GUI 主程序: {}。请确保 aether 和 {} 在同一目录。",
            app_exe.display(),
            app_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    /// 临时目录，测试结束时自动清理
    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new(name: &str) -> Self {
            let dir = env::temp_dir().join(format!("{}-{}", name, std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 保证测试结束后恢复原来的工作目录
    struct ChdirGuard(PathBuf);

    impl Drop for ChdirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.0);
        }
    }

    // ==================== parse_goto_arg ====================

    #[test]
    fn test_parse_goto_arg_none() {
        assert_eq!(parse_goto_arg(None).unwrap(), (None, None));
    }

    #[test]
    fn test_parse_goto_arg_line_only() {
        let (file, pos) = parse_goto_arg(Some("10".to_string())).unwrap();
        assert_eq!(file, None);
        assert_eq!(
            pos,
            Some(GotoPosition {
                line: 10,
                column: 1
            })
        );
    }

    #[test]
    fn test_parse_goto_arg_line_and_column() {
        let (file, pos) = parse_goto_arg(Some("10:5".to_string())).unwrap();
        assert_eq!(file, None);
        assert_eq!(
            pos,
            Some(GotoPosition {
                line: 10,
                column: 5
            })
        );
    }

    #[test]
    fn test_parse_goto_arg_with_file() {
        let (file, pos) = parse_goto_arg(Some("file.txt:12:3".to_string())).unwrap();
        assert_eq!(file, Some(PathBuf::from("file.txt")));
        assert_eq!(
            pos,
            Some(GotoPosition {
                line: 12,
                column: 3
            })
        );
    }

    #[test]
    fn test_parse_goto_arg_errors() {
        assert!(parse_goto_arg(Some("".to_string())).is_err());
        assert!(parse_goto_arg(Some("file.txt".to_string())).is_err());
        assert!(parse_goto_arg(Some("0:5".to_string())).is_err());
        assert!(parse_goto_arg(Some("file.txt:abc:xyz".to_string())).is_err());
    }

    // ==================== normalize_paths ====================

    #[test]
    fn test_normalize_paths_empty() {
        assert!(normalize_paths(vec![]).unwrap().is_empty());
    }

    #[test]
    fn test_normalize_paths_nonexistent_relative() {
        let cwd = env::current_dir().unwrap();
        let res = normalize_paths(vec![PathBuf::from("not-there.txt")]).unwrap();
        assert_eq!(res, vec![cwd.join("not-there.txt")]);
    }

    #[test]
    fn test_normalize_paths_absolute_unchanged() {
        let abs = env::current_dir().unwrap().join("Cargo.toml");
        let res = normalize_paths(vec![abs.clone()]).unwrap();
        assert_eq!(res, vec![abs]);
    }

    #[test]
    fn test_normalize_paths_existing_relative() {
        let tmp = TestTempDir::new("normalize-existing");
        let file = tmp.path().join("file.txt");
        fs::write(&file, "hello").unwrap();

        let old = env::current_dir().unwrap();
        let _guard = ChdirGuard(old);
        env::set_current_dir(tmp.path()).unwrap();

        let res = normalize_paths(vec![PathBuf::from("file.txt")]).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], file.canonicalize().unwrap_or(file.clone()));
    }

    // ==================== find_app_exe ====================

    #[test]
    fn test_find_app_exe_found() {
        let tmp = TestTempDir::new("find-app-found");
        let app_name = if cfg!(windows) {
            "aether-app.exe"
        } else {
            "aether-app"
        };
        let app = tmp.path().join(app_name);
        fs::write(&app, "").unwrap();

        let dummy_exe = tmp.path().join("dummy.exe");
        let found = find_app_exe_in(&dummy_exe).unwrap();
        assert_eq!(found, app);
    }

    #[test]
    fn test_find_app_exe_not_found() {
        let tmp = TestTempDir::new("find-app-missing");
        let dummy_exe = tmp.path().join("dummy.exe");
        let err = find_app_exe_in(&dummy_exe).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("aether-app"),
            "错误信息应包含主程序名: {}",
            msg
        );
        assert!(msg.contains("找不到"), "错误信息应提示找不到: {}", msg);
    }

    // ==================== Cli parsing ====================

    #[test]
    fn test_cli_parse_no_args() {
        let cli = Cli::try_parse_from(["aether"]).unwrap();
        assert!(cli.paths.is_empty());
        assert!(!cli.new_window);
        assert!(!cli.wait);
        assert!(cli.goto.is_none());
    }

    #[test]
    fn test_cli_parse_full() {
        let cli = Cli::try_parse_from([
            "aether",
            "file.txt",
            "--new-window",
            "--wait",
            "--goto",
            "10:5",
        ])
        .unwrap();
        assert_eq!(cli.paths, vec![PathBuf::from("file.txt")]);
        assert!(cli.new_window);
        assert!(cli.wait);
        assert_eq!(cli.goto, Some("10:5".to_string()));
    }

    // ==================== build_launch_args ====================

    #[test]
    fn test_build_launch_args_no_goto() {
        let cwd = env::current_dir().unwrap();
        let cli = Cli {
            paths: vec![PathBuf::from("foo.txt")],
            new_window: false,
            wait: false,
            goto: None,
        };
        let args = build_launch_args(&cli).unwrap();
        assert_eq!(args.paths, vec![cwd.join("foo.txt")]);
        assert!(!args.new_window);
        assert!(!args.wait);
        assert!(args.goto.is_none());
    }

    #[test]
    fn test_build_launch_args_goto_file_inserts() {
        let cwd = env::current_dir().unwrap();
        let cli = Cli {
            paths: vec![PathBuf::from("foo.txt")],
            new_window: true,
            wait: true,
            goto: Some("bar.txt:3:2".to_string()),
        };
        let args = build_launch_args(&cli).unwrap();
        assert_eq!(
            args.paths,
            vec![PathBuf::from("bar.txt"), cwd.join("foo.txt")]
        );
        assert!(args.new_window);
        assert!(args.wait);
        assert_eq!(args.goto, Some(GotoPosition { line: 3, column: 2 }));
    }

    #[test]
    fn test_build_launch_args_goto_file_moves_to_front() {
        let cwd = env::current_dir().unwrap();
        let abs = cwd.join("shared.txt");
        let goto_str = format!("{}:5:1", abs.to_string_lossy());
        let cli = Cli {
            paths: vec![abs.clone()],
            new_window: false,
            wait: false,
            goto: Some(goto_str),
        };
        let args = build_launch_args(&cli).unwrap();
        assert_eq!(args.paths, vec![abs]);
        assert_eq!(args.goto, Some(GotoPosition { line: 5, column: 1 }));
    }

    #[test]
    fn test_build_launch_args_goto_position_only() {
        let cwd = env::current_dir().unwrap();
        let cli = Cli {
            paths: vec![PathBuf::from("foo.txt")],
            new_window: false,
            wait: false,
            goto: Some("12:8".to_string()),
        };
        let args = build_launch_args(&cli).unwrap();
        assert_eq!(args.paths, vec![cwd.join("foo.txt")]);
        assert_eq!(
            args.goto,
            Some(GotoPosition {
                line: 12,
                column: 8
            })
        );
    }
}
