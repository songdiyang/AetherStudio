use std::io;
use std::panic;
use std::path::PathBuf;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志系统
///
/// - 日志文件按天轮转，写入 `%APPDATA%/Aether/logs/aether-YYYY-MM-DD.log`
/// - 控制台输出（debug 及以上）
/// - panic 时自动 flush 日志
/// - 支持 `RUST_LOG` 环境变量覆盖日志级别
pub fn init_logging() -> io::Result<()> {
    // 1. 确定日志目录：%APPDATA%/Aether/logs
    let log_dir = get_log_dir();
    std::fs::create_dir_all(&log_dir)?;

    // 2. 创建按日轮转的文件 appender
    let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "aether");

    // 3. 使用本地时区格式化时间
    let local_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let timer = OffsetTime::new(
        local_offset,
        time::format_description::parse_borrowed::<1>(
            "[year]-[month]-[day] [hour]:[minute]:[second]",
        )
        .unwrap_or_else(|_| {
            time::format_description::parse_borrowed::<1>("[year]-[month]-[day]").unwrap()
        }),
    );

    // 4. 文件日志层（含时间戳、级别、目标模块）
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_timer(timer.clone())
        .with_ansi(false)
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(true)
        .with_file(true);

    // 5. 控制台日志层（debug 构建时启用）
    let console_layer = tracing_subscriber::fmt::layer()
        .with_timer(timer)
        .with_ansi(true)
        .with_level(true)
        .with_target(true)
        .with_line_number(true)
        .with_file(true);

    // 6. 日志级别过滤（默认 info，可通过 RUST_LOG 覆盖）
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 7. 注册订阅者
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    // 8. 安装 panic hook，确保崩溃前 flush 日志
    install_panic_hook();

    tracing::info!(log_dir = %log_dir.display(), "日志系统初始化完成");
    Ok(())
}

/// 获取日志目录路径
fn get_log_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("Aether")
        .join("logs")
}

/// 安装 panic hook，在崩溃前尝试 flush 日志并记录 panic 信息
fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // 记录 panic 信息到日志
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "未知 panic payload".to_string()
        };

        let location = if let Some(loc) = info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "未知位置".to_string()
        };

        tracing::error!(
            panic.payload = %payload,
            panic.location = %location,
            "应用程序发生 panic"
        );

        // 强制 flush 日志写入器
        let _ = std::io::Write::flush(&mut std::io::stdout());

        // 调用默认 hook（输出到控制台/Windows 错误报告）
        default_hook(info);
    }));
}

/// 手动 flush 日志（用于关键操作后确保日志落地）
pub fn flush_logs() {
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
