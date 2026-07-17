use super::*;

impl EditorState {
    /// P0-2: 设置 IME 合成串（pre-edit text）。
    /// 在 WM_IME_COMPOSITION 收到 GCS_COMPSTR 时调用，
    /// 清空已存在的合成串后写入新值，并触发重绘。
    pub fn set_composition(&mut self, text: String) {
        // 修复：file_tree_input 激活时，合成串存到输入框而非编辑器
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.composition = Some(text);
                input.caret_visible = true;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，合成串存到 AI 面板
        if self.ai_panel.input_focused {
            self.ai_panel.composition = Some(text);
            self.ai_panel.caret_visible = true;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        self.composition = Some(text);
    }
    /// 终端聚焦时关闭 IME（保留关联），让 Backspace 直达终端。
    /// 失去焦点时根据用户偏好决定是否恢复 IME —— 通常让用户切回编辑器时仍能输入中文。
    /// 同时同步更新低层键盘钩子的 `TERMINAL_FOCUSED_FLAG` 标志，
    /// 让低层钩子在终端聚焦时主动拦截 Backspace/Delete/方向键。
    ///
    /// 多语言 IME 适配说明：
    /// - 终端聚焦时 set_ime_open(false) 关闭"已开启未合成"状态对 Backspace 的拦截
    /// - 低层键盘钩子 (WH_KEYBOARD_LL) 在所有 IME 之上拦截 Backspace/Delete/方向键
    /// - 字符输入仍走 WM_IME_COMPOSITION + GCS_RESULTSTR → commit_composition → ConPTY
    /// - 因此这个方案对中文/日文/韩文/印地/泰文/阿拉伯等所有标准 IME 通用
    pub fn set_terminal_ime_bypass(&mut self, terminal_focused: bool) {
        // 终端聚焦时关闭 IME，编辑器聚焦时恢复 IME 开启状态
        // （之前总是传 false，会导致用户切回编辑器时中文无法输入）
        let ime_open = !terminal_focused;
        let ok = self.ime.set_ime_open(ime_open);
        crate::keyboard_hook::set_terminal_focused(terminal_focused);
        tracing::info!(
            terminal_focused,
            ime_open,
            ok,
            "终端 IME 状态切换完成，低层钩子标志已同步"
        );
    }
    /// P0-2: 提交合成串为正式文本。
    /// 在 WM_IME_COMPOSITION 收到 GCS_RESULTSTR 或 WM_IME_ENDCOMPOSITION 时调用。
    /// 先清除合成串，再将提交文本逐字符插入到光标处。
    ///
    /// 修复：终端聚焦时，IME 提交文本路由到 ConPTY 而非编辑器。
    /// 中文/日文 IME 在终端聚焦时仍会拦截 ASCII 字母做合成（因为窗口级 IME 关联未变），
    /// 提交结果必须进入终端，否则用户输入的字母会被"偷"到编辑器。
    pub fn commit_composition(&mut self, text: String) {
        // 清除合成状态显示
        self.composition = None;
        if text.is_empty() {
            return;
        }
        // 终端聚焦且运行时：把 IME 提交文本送入 ConPTY
        if self.terminal_panel.focused && self.terminal_panel.running {
            for ch in text.chars() {
                self.terminal_panel.send_char(ch);
            }
            // 提交后立即关闭 IME，让用户能立刻用 Backspace 删除刚提交的汉字
            // （否则 IME 处于"开启未合成"状态会系统级拦截 Backspace）
            self.ime.set_ime_open(false);
            return;
        }
        if self.new_project_dialog.visible {
            self.new_project_dialog.project_name.push_str(&text);
            self.new_project_dialog.error_message = None;
            return;
        }
        // 修复：file_tree_input 激活时，IME 提交文本应进入输入框而非编辑器内容
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.value.push_str(&text);
                input.composition = None;
                input.caret_visible = true;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，IME 提交文本进入 AI 输入框
        if self.ai_panel.input_focused {
            self.ai_panel.insert_str(&text);
            self.ai_panel.composition = None;
            self.ai_panel.caret_visible = true;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        for ch in text.chars() {
            self.broadcast_insert_char(ch);
        }
    }
    /// P0-2: 清除合成串（用户取消输入或 IME 失焦时调用）。
    pub fn clear_composition(&mut self) {
        // 修复：file_tree_input 激活时，清除输入框的合成串
        if self.file_tree_input.is_some() {
            if let Some(input) = self.file_tree_input.as_mut() {
                input.composition = None;
            }
            let region = self.layout.sidebar_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::Sidebar,
            );
            return;
        }
        // AI 面板输入框聚焦时，清除 AI 面板的合成串
        if self.ai_panel.input_focused {
            self.ai_panel.composition = None;
            let region = self.layout.right_panel_region().clone();
            self.dirty_tracker.mark_region(
                region.x,
                region.y,
                region.width,
                region.height,
                crate::dirty_rect::DirtyRegionType::RightPanel,
            );
            return;
        }
        self.composition = None;
    }
}
