/// GUI 命中测试 / 自动化辅助模块
///
/// 记录当前帧所有可点击区域的名称与矩形，输出到文件供外部测试框架读取。
/// 这些数据只在调试/测试构建中启用，避免 release 性能开销。
///
/// REQ-P2-10: release 构建中所有 Mutex 锁和文件 I/O 均被 cfg 门控移除，
/// register_hit_region / clear_hit_regions / flush_hit_regions_to_file 在
/// release 构建中编译为空实现，零运行时开销。
use std::io::Write;

/// 单个可点击区域
#[derive(Clone, Debug, serde::Serialize)]
pub struct HitRegion {
    pub action: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 一帧的命中区域集合
#[derive(Default, Debug)]
pub struct HitTestFrame {
    pub regions: Vec<HitRegion>,
}

impl HitTestFrame {
    pub fn new() -> Self {
        Self {
            regions: Vec::with_capacity(64),
        }
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn add(&mut self, action: impl Into<String>, x: f32, y: f32, width: f32, height: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        self.regions.push(HitRegion {
            action: action.into(),
            x,
            y,
            width,
            height,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

// ============================================================================
// REQ-P2-10: debug 构建保留全局 Mutex + 文件 I/O，release 构建完全移除
// ============================================================================
#[cfg(debug_assertions)]
mod debug_impl {
    use super::*;
    use std::sync::Mutex;

    /// 全局帧记录（简化：每帧覆盖）
    static HIT_TEST_FRAME: Mutex<Option<HitTestFrame>> = Mutex::new(None);

    /// 注册一个可点击区域（线程安全）
    pub fn register_hit_region(action: impl Into<String>, x: f32, y: f32, width: f32, height: f32) {
        if let Ok(mut guard) = HIT_TEST_FRAME.lock() {
            let frame = guard.get_or_insert_with(HitTestFrame::new);
            frame.add(action, x, y, width, height);
        }
    }

    /// 清除本帧记录（通常在 render 开始时调用）
    pub fn clear_hit_regions() {
        if let Ok(mut guard) = HIT_TEST_FRAME.lock() {
            if let Some(frame) = guard.as_mut() {
                frame.clear();
            }
        }
    }

    /// 将当前帧区域写入 JSONL 文件
    ///
    /// 路径：项目根目录 `tests/gui_hit_regions.jsonl`
    /// 每行一个 JSON 对象：`{"action":"...","x":...,"y":...,"width":...,"height":...}`
    pub fn flush_hit_regions_to_file() {
        let regions = {
            let Ok(mut guard) = HIT_TEST_FRAME.lock() else {
                return;
            };
            guard.take().map(|f| f.regions).unwrap_or_default()
        };

        if regions.is_empty() {
            return;
        }

        let path = hit_regions_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(_) => return,
        };

        for region in regions {
            if let Ok(json) = serde_json::to_string(&region) {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    fn hit_regions_path() -> std::path::PathBuf {
        // 项目根目录：从 executable 往上找 Cargo.toml
        let mut dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // 向上查找 Cargo.toml
        loop {
            if dir.join("Cargo.toml").exists() {
                return dir.join("tests").join("gui_hit_regions.jsonl");
            }
            if !dir.pop() {
                break;
            }
        }

        std::env::current_dir()
            .unwrap_or_default()
            .join("tests")
            .join("gui_hit_regions.jsonl")
    }
}

// ============================================================================
// REQ-P2-10: release 构建使用空实现，零 Mutex 锁竞争和文件 I/O 开销
// ============================================================================
#[cfg(not(debug_assertions))]
mod release_impl {
    use super::*;

    #[inline]
    pub fn register_hit_region(
        _action: impl Into<String>,
        _x: f32,
        _y: f32,
        _width: f32,
        _height: f32,
    ) {
        // release 构建：空操作，零开销
    }

    #[inline]
    pub fn clear_hit_regions() {
        // release 构建：空操作，零开销
    }

    #[inline]
    pub fn flush_hit_regions_to_file() {
        // release 构建：空操作，零开销
    }
}

// 公共 API：根据构建模式路由到对应实现
#[cfg(debug_assertions)]
pub use debug_impl::{clear_hit_regions, flush_hit_regions_to_file, register_hit_region};

#[cfg(not(debug_assertions))]
pub use release_impl::{clear_hit_regions, flush_hit_regions_to_file, register_hit_region};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_new_is_empty() {
        let frame = HitTestFrame::new();
        assert!(frame.is_empty());
        assert_eq!(frame.regions.len(), 0);
    }

    #[test]
    fn test_frame_add_region() {
        let mut frame = HitTestFrame::new();
        frame.add("menu:file", 10.0, 20.0, 80.0, 24.0);
        assert!(!frame.is_empty());
        assert_eq!(frame.regions.len(), 1);
        assert_eq!(frame.regions[0].action, "menu:file");
        assert_eq!(frame.regions[0].x, 10.0);
    }

    #[test]
    fn test_frame_add_ignores_non_positive_size() {
        let mut frame = HitTestFrame::new();
        frame.add("zero-w", 0.0, 0.0, 0.0, 10.0);
        frame.add("zero-h", 0.0, 0.0, 10.0, 0.0);
        frame.add("neg-w", 0.0, 0.0, -1.0, 10.0);
        frame.add("neg-h", 0.0, 0.0, 10.0, -1.0);
        assert!(frame.is_empty(), "零或负尺寸的区域应被忽略");
    }

    #[test]
    fn test_frame_clear() {
        let mut frame = HitTestFrame::new();
        frame.add("a", 0.0, 0.0, 1.0, 1.0);
        frame.add("b", 0.0, 0.0, 1.0, 1.0);
        assert_eq!(frame.regions.len(), 2);
        frame.clear();
        assert!(frame.is_empty());
    }

    #[test]
    fn test_register_and_clear_global_regions() {
        // 全局 Mutex 状态可能在其他测试间共享，这里只验证调用不 panic 且语义自洽
        register_hit_region("test:region", 1.0, 2.0, 3.0, 4.0);
        register_hit_region("test:region2", 5.0, 6.0, 7.0, 8.0);
        // 清除不应 panic
        clear_hit_regions();
    }

    #[test]
    fn test_hit_region_serializes_to_json() {
        let region = HitRegion {
            action: "menu:file".to_string(),
            x: 1.5,
            y: 2.5,
            width: 80.0,
            height: 24.0,
        };
        let json = serde_json::to_string(&region).expect("应可序列化");
        assert!(json.contains("\"action\":\"menu:file\""));
        assert!(json.contains("1.5"));
    }
}
