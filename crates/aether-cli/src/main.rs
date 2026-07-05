use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::Parser;

use aether_shared::launch::{parse_goto, GotoPosition, LaunchArgs};

#[derive(Parser, Debug)]
#[command(
    name = "aether",
    about = "Aether Editor 命令行接口",
    version
)]
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

    let (goto_file, goto_position) = parse_goto_arg(cli.goto)?;

    let mut paths = normalize_paths(cli.paths)?;
    if let Some(file) = goto_file {
        // 如果 goto 中指定了文件，把它放到路径列表最前面，确保优先加载并跳转
        if let Some(pos) = paths.iter().position(|p| p == &file) {
            paths.remove(pos);
        }
        paths.insert(0, file);
    }

    let args = LaunchArgs {
        paths,
        new_window: cli.new_window,
        goto: goto_position,
        wait: cli.wait,
    };

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
    let dir = current_exe
        .parent()
        .context("无法获取可执行文件所在目录")?;

    let app_name = if cfg!(windows) { "aether-app.exe" } else { "aether-app" };
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
