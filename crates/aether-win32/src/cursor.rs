//! 鼠标光标语义化
//!
//! 根据 hover 区域返回不同的光标类型，由 `WM_SETCURSOR` 调用 `LoadCursorW` + `SetCursor`。
//! `mouse_move.rs` 仅暴露 `compute_cursor_for_pos` 计算光标类型，不直接调用 `SetCursor`。

/// 光标类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorType {
    #[default]
    Arrow,
    IBeam,
    Hand,
    SizeWE,
    SizeNS,
}

impl CursorType {
    /// 返回对应的 IDC_* 光标资源常量（windows crate 中的 PCWSTR）
    pub fn idc_cursor(self) -> windows::core::PCWSTR {
        use windows::Win32::UI::WindowsAndMessaging::*;
        match self {
            CursorType::Arrow => IDC_ARROW,
            CursorType::IBeam => IDC_IBEAM,
            CursorType::Hand => IDC_HAND,
            CursorType::SizeWE => IDC_SIZEWE,
            CursorType::SizeNS => IDC_SIZENS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_arrow() {
        let c = CursorType::default();
        assert_eq!(c, CursorType::Arrow);
    }

    #[test]
    fn test_idc_cursor_mapping() {
        use windows::Win32::UI::WindowsAndMessaging::{
            IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZENS, IDC_SIZEWE,
        };
        assert_eq!(CursorType::Arrow.idc_cursor(), IDC_ARROW);
        assert_eq!(CursorType::IBeam.idc_cursor(), IDC_IBEAM);
        assert_eq!(CursorType::Hand.idc_cursor(), IDC_HAND);
        assert_eq!(CursorType::SizeWE.idc_cursor(), IDC_SIZEWE);
        assert_eq!(CursorType::SizeNS.idc_cursor(), IDC_SIZENS);
    }

    #[test]
    fn test_copy_and_eq() {
        let a = CursorType::IBeam;
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, CursorType::Arrow);
    }
}
