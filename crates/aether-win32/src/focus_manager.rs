//! REQ-P0-05: 统一焦点管理
//!
//! 集中管理窗口内各面板（编辑器、终端、AI 面板、查找替换等）的焦点状态。
//! 通过 WM_SETFOCUS / WM_KILLFOCUS 消息同步窗口级焦点，
//! 并维护焦点历史栈以支持焦点回退（如关闭查找面板后焦点回到编辑器）。
//!
//! 此阶段（T01）仅实现基础结构和窗口级焦点消息处理。
//! 完整的键盘事件路由将在 T03 中完善。

/// 查找替换面板内部的焦点子目标
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindReplaceFocus {
    None,
    FindQuery,
    ReplaceText,
}

/// 焦点目标枚举 —— 标识当前接收键盘输入的面板
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// 编辑器（默认焦点）
    Editor,
    /// 底部终端面板
    Terminal,
    /// 右侧 AI 助手面板
    AiPanel,
    /// 查找替换面板（含子焦点状态）
    FindReplace(FindReplaceFocus),
    /// 命令面板
    CommandPalette,
    /// 设置面板
    Settings,
    /// 对话框（SSH、克隆等模态对话框）
    Dialog,
    /// 无焦点（窗口失焦时）
    None,
}

impl Default for FocusTarget {
    fn default() -> Self {
        FocusTarget::Editor
    }
}

/// 统一焦点管理器
///
/// 维护当前焦点目标和焦点历史栈。
/// 当窗口失去焦点时（WM_KILLFOCUS），`current()` 返回 `FocusTarget::None`。
/// 当窗口重新获得焦点时（WM_SETFOCUS），恢复之前的焦点目标。
pub struct FocusManager {
    /// 当前焦点目标（窗口失焦时仍保留此值，通过 current() 返回 None）
    current: FocusTarget,
    /// 焦点历史栈，用于 push/pop 焦点回退
    history: Vec<FocusTarget>,
    /// 窗口是否拥有焦点
    window_focused: bool,
}

impl FocusManager {
    /// 创建新的焦点管理器，默认焦点为编辑器
    pub fn new() -> Self {
        Self {
            current: FocusTarget::Editor,
            history: Vec::with_capacity(8),
            window_focused: true,
        }
    }

    /// 获取当前焦点目标
    /// 窗口失焦时返回 `FocusTarget::None`
    pub fn current(&self) -> FocusTarget {
        if !self.window_focused {
            FocusTarget::None
        } else {
            self.current
        }
    }

    /// 直接设置焦点目标（不清除历史栈）
    pub fn set(&mut self, target: FocusTarget) {
        self.current = target;
    }

    /// 压入焦点目标（保存当前焦点到历史栈，然后切换）
    pub fn push(&mut self, target: FocusTarget) {
        self.history.push(self.current);
        self.current = target;
    }

    /// 弹出焦点目标（恢复到上一个焦点）
    /// 返回恢复后的焦点目标，如果历史栈为空则返回 None 并回退到编辑器
    pub fn pop(&mut self) -> Option<FocusTarget> {
        if let Some(prev) = self.history.pop() {
            self.current = prev;
            Some(self.current)
        } else {
            self.current = FocusTarget::Editor;
            None
        }
    }

    /// WM_SETFOCUS 处理：窗口获得焦点
    pub fn on_set_focus(&mut self) {
        self.window_focused = true;
    }

    /// WM_KILLFOCUS 处理：窗口失去焦点
    pub fn on_kill_focus(&mut self) {
        self.window_focused = false;
    }

    /// 窗口是否拥有焦点
    pub fn is_window_focused(&self) -> bool {
        self.window_focused
    }

    /// 清除焦点历史栈
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_focus_is_editor() {
        let fm = FocusManager::new();
        assert_eq!(fm.current(), FocusTarget::Editor);
    }

    #[test]
    fn test_set_focus() {
        let mut fm = FocusManager::new();
        fm.set(FocusTarget::Terminal);
        assert_eq!(fm.current(), FocusTarget::Terminal);
    }

    #[test]
    fn test_push_pop_focus() {
        let mut fm = FocusManager::new();
        assert_eq!(fm.current(), FocusTarget::Editor);

        fm.push(FocusTarget::FindReplace(FindReplaceFocus::FindQuery));
        assert_eq!(
            fm.current(),
            FocusTarget::FindReplace(FindReplaceFocus::FindQuery)
        );

        let restored = fm.pop();
        assert_eq!(restored, Some(FocusTarget::Editor));
        assert_eq!(fm.current(), FocusTarget::Editor);
    }

    #[test]
    fn test_pop_empty_returns_none() {
        let mut fm = FocusManager::new();
        fm.set(FocusTarget::Terminal);
        let restored = fm.pop();
        assert_eq!(restored, None);
        assert_eq!(fm.current(), FocusTarget::Editor);
    }

    #[test]
    fn test_window_focus() {
        let mut fm = FocusManager::new();
        assert!(fm.is_window_focused());
        assert_eq!(fm.current(), FocusTarget::Editor);

        fm.on_kill_focus();
        assert!(!fm.is_window_focused());
        assert_eq!(fm.current(), FocusTarget::None);

        fm.on_set_focus();
        assert!(fm.is_window_focused());
        assert_eq!(fm.current(), FocusTarget::Editor);
    }

    #[test]
    fn test_window_focus_preserves_current() {
        let mut fm = FocusManager::new();
        fm.set(FocusTarget::CommandPalette);

        fm.on_kill_focus();
        assert_eq!(fm.current(), FocusTarget::None);

        fm.on_set_focus();
        assert_eq!(fm.current(), FocusTarget::CommandPalette);
    }
}
