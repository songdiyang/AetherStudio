/// GUI 命中测试 / 自动化辅助模块
///
/// 记录当前帧所有可点击区域的名称与矩形，输出到文件供外部测试框架读取。
/// 这些数据只在调试/测试构建中启用，避免 release 性能开销。
use std::io::Write;
use std::sync::Mutex;

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

/// 全局帧记录（简化：每帧覆盖）
static HIT_TEST_FRAME: Mutex<Option<HitTestFrame>> = Mutex::new(None);

/// 注册一个可点击区域（线程安全）
pub fn register_hit_region(
    action: impl Into<String>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
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
