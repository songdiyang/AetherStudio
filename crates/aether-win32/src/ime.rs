use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::Ime::{
    ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow,
    ImmSetCompositionWindow, CANDIDATEFORM, CFS_CANDIDATEPOS, CFS_POINT, COMPOSITIONFORM,
    IME_COMPOSITION_STRING,
};

/// GCS_COMPSTR：合成串（pre-edit text）查询标志
const GCS_COMPSTR: u32 = 0x0008;
/// GCS_RESULTSTR：结果串（已提交文本）查询标志
const GCS_RESULTSTR: u32 = 0x0800;

/// REQ-P2-04: 候选窗口基础尺寸（96 DPI 基准，单位：物理像素）
/// 候选窗口宽度（容纳 4-6 个汉字候选词）
const CANDIDATE_BASE_WIDTH: i32 = 220;
/// 候选窗口高度（容纳 5-9 条候选 + 标题栏）
const CANDIDATE_BASE_HEIGHT: i32 = 200;
/// 候选窗口顶部偏移（位于光标下方若干像素，避免遮挡光标）
const CANDIDATE_TOP_OFFSET: i32 = 4;
/// 合成窗口基础宽度（容纳一行预编辑文本）
const COMPOSITION_BASE_WIDTH: i32 = 400;
/// 合成窗口基础高度
const COMPOSITION_BASE_HEIGHT: i32 = 60;

/// TSF (Text Services Framework) / IMM32 输入法集成
/// 支持中文、日文、韩文等 CJK 输入法的候选窗口定位
///
/// REQ-P2-04: 候选窗口尺寸随 DPI 缩放，确保高 DPI 显示器上候选窗口可读
pub struct ImeIntegration {
    hwnd: HWND,
    enabled: bool,
    /// REQ-P2-04: DPI 缩放因子（1.0 = 96 DPI），用于缩放候选/合成窗口尺寸
    dpi_scale: f32,
    /// REQ-P2-04: 编辑器字体大小（逻辑像素），用于设置合成窗口字体
    font_size: f32,
    /// REQ-P2-04: 编辑器字体名称，用于设置合成窗口字体
    font_name: String,
}

impl ImeIntegration {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            enabled: true,
            dpi_scale: 1.0,
            font_size: 14.0,
            font_name: "Consolas".to_string(),
        }
    }

    /// REQ-P2-04: 设置 DPI 缩放因子
    /// 在窗口初始化和 WM_DPICHANGED 时调用
    pub fn set_dpi_scale(&mut self, scale: f32) {
        self.dpi_scale = scale.max(0.5);
    }

    /// REQ-P2-04: 设置编辑器字体信息，用于合成窗口字体匹配
    pub fn set_font(&mut self, font_size: f32, font_name: &str) {
        self.font_size = font_size.max(8.0);
        self.font_name = font_name.to_string();
    }

    /// 设置输入法候选窗口位置（跟随光标）
    /// REQ-P2-04: 候选窗口 rcArea 尺寸按 dpi_scale 缩放
    pub fn set_candidate_window_position(&self, x: i32, y: i32) {
        if !self.enabled {
            return;
        }

        let scale = self.dpi_scale;
        let width = (CANDIDATE_BASE_WIDTH as f32 * scale) as i32;
        let height = (CANDIDATE_BASE_HEIGHT as f32 * scale) as i32;
        let top_offset = (CANDIDATE_TOP_OFFSET as f32 * scale) as i32;

        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if !himc.0.is_null() {
                let candidate_form = CANDIDATEFORM {
                    dwIndex: 0,
                    dwStyle: CFS_CANDIDATEPOS,
                    ptCurrentPos: windows::Win32::Foundation::POINT { x, y },
                    rcArea: RECT {
                        left: x,
                        top: y + top_offset,
                        right: x + width,
                        bottom: y + top_offset + height,
                    },
                };
                let _ = ImmSetCandidateWindow(himc, &candidate_form);
                let _ = ImmReleaseContext(self.hwnd, himc);
            }
        }
    }

    /// 设置合成窗口位置（预编辑文本位置）
    /// REQ-P2-04: 合成窗口 rcArea 尺寸按 dpi_scale 缩放
    pub fn set_composition_window_position(&self, x: i32, y: i32) {
        if !self.enabled {
            return;
        }

        let scale = self.dpi_scale;
        let width = (COMPOSITION_BASE_WIDTH as f32 * scale) as i32;
        let height = (COMPOSITION_BASE_HEIGHT as f32 * scale) as i32;

        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if !himc.0.is_null() {
                let comp_form = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: windows::Win32::Foundation::POINT { x, y },
                    rcArea: RECT {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + height,
                    },
                };
                let _ = ImmSetCompositionWindow(himc, &comp_form);
                let _ = ImmReleaseContext(self.hwnd, himc);
            }
        }
    }

    /// 更新 IME 位置（在光标移动时调用）
    /// REQ-P2-04: 坐标参数应为物理像素（调用方按 dpi_scale 转换）
    pub fn update_ime_position(&self, cursor_x: f32, cursor_y: f32, line_height: f32) {
        let x = cursor_x as i32;
        let y = cursor_y as i32;
        let bottom = (cursor_y + line_height) as i32;

        self.set_composition_window_position(x, y);
        self.set_candidate_window_position(x, bottom);
    }

    /// 读取 IME 合成串（pre-edit text）。
    /// 在 WM_IME_COMPOSITION 收到 GCS_COMPSTR 标志时调用。
    pub fn get_composition_string(&self) -> Option<String> {
        self.get_ime_string(GCS_COMPSTR)
    }

    /// 读取 IME 结果串（已提交文本）。
    /// 在 WM_IME_COMPOSITION 收到 GCS_RESULTSTR 标志时调用。
    pub fn get_result_string(&self) -> Option<String> {
        self.get_ime_string(GCS_RESULTSTR)
    }

    /// 通用：按标志从 ImmGetCompositionStringW 读取 UTF-16 字符串。
    fn get_ime_string(&self, flag: u32) -> Option<String> {
        if !self.enabled {
            return None;
        }

        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.0.is_null() {
                return None;
            }

            // 释放 HIMC 的 RAII 守卫
            let _guard = HimcGuard(self.hwnd, himc);

            let flag = IME_COMPOSITION_STRING(flag);

            // 第一次调用获取字节长度（含结尾 NUL）
            let byte_len = ImmGetCompositionStringW(himc, flag, None, 0);
            if byte_len <= 0 {
                return None;
            }

            let wchar_count = (byte_len as usize) / 2;
            let mut buf: Vec<u16> = vec![0u16; wchar_count];

            // 第二次调用填充缓冲区
            let written = ImmGetCompositionStringW(
                himc,
                flag,
                Some(buf.as_mut_ptr() as *mut _),
                byte_len as u32,
            );
            if written <= 0 {
                return None;
            }

            // 去掉结尾 NUL（若有）
            let actual_len = (written as usize) / 2;
            while buf.len() > actual_len {
                buf.pop();
            }
            while buf.last() == Some(&0) {
                buf.pop();
            }

            String::from_utf16(&buf).ok()
        }
    }

    /// 是否启用 IME 集成
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 启用/禁用 IME 集成
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// RAII 守卫：确保 ImmReleaseContext 在作用域结束时被调用
struct HimcGuard(HWND, windows::Win32::UI::Input::Ime::HIMC);

impl Drop for HimcGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ImmReleaseContext(self.0, self.1);
        }
    }
}
