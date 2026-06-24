use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::Ime::{
    ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
    CANDIDATEFORM, CFS_CANDIDATEPOS, CFS_POINT, COMPOSITIONFORM,
};

/// TSF (Text Services Framework) / IMM32 输入法集成
/// 支持中文、日文、韩文等 CJK 输入法的候选窗口定位
pub struct ImeIntegration {
    hwnd: HWND,
    enabled: bool,
}

impl ImeIntegration {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            enabled: true,
        }
    }

    /// 设置输入法候选窗口位置（跟随光标）
    pub fn set_candidate_window_position(&self, x: i32, y: i32) {
        if !self.enabled {
            return;
        }

        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if !himc.0.is_null() {
                let candidate_form = CANDIDATEFORM {
                    dwIndex: 0,
                    dwStyle: CFS_CANDIDATEPOS,
                    ptCurrentPos: windows::Win32::Foundation::POINT { x, y },
                    rcArea: RECT {
                        left: x,
                        top: y + 20,
                        right: x + 200,
                        bottom: y + 220,
                    },
                };
                let _ = ImmSetCandidateWindow(himc, &candidate_form);
                let _ = ImmReleaseContext(self.hwnd, himc);
            }
        }
    }

    /// 设置合成窗口位置（预编辑文本位置）
    pub fn set_composition_window_position(&self, x: i32, y: i32) {
        if !self.enabled {
            return;
        }

        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if !himc.0.is_null() {
                let comp_form = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: windows::Win32::Foundation::POINT { x, y },
                    rcArea: RECT {
                        left: x,
                        top: y,
                        right: x + 400,
                        bottom: y + 200,
                    },
                };
                let _ = ImmSetCompositionWindow(himc, &comp_form);
                let _ = ImmReleaseContext(self.hwnd, himc);
            }
        }
    }

    /// 更新 IME 位置（在光标移动时调用）
    pub fn update_ime_position(&self, cursor_x: f32, cursor_y: f32, line_height: f32) {
        let x = cursor_x as i32;
        let y = cursor_y as i32;
        let bottom = (cursor_y + line_height) as i32;

        self.set_composition_window_position(x, y);
        self.set_candidate_window_position(x, bottom);
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
