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
