//! 键盘事件处理模块。
//!
//! 从 `window.rs` 拆分而来，保持原有逻辑不变。
//! 包含 WM_CHAR 和 WM_KEYDOWN 处理；大型函数拆分到子模块中以控制单文件行数。

mod char_input;
mod key_down;
mod key_down_ctrl;
mod key_down_edit;

pub(crate) use char_input::on_char;
pub(crate) use key_down::on_key_down;
