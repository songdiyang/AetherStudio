//! 自动保存（Auto-Save）模块
//!
//! 设计目标：用户无感知、数据零丢失、性能零干扰、行为可预期。
//!
//! ## 触发策略（组合式）
//! 1. **空闲防抖保存（Debounce）** —— 用户停止输入后延迟固定时间保存；
//!    输入过程中不断重置计时器。
//! 2. **失焦立即保存** —— 编辑器失去焦点时立即保存。
//! 3. **周期强制保存（兜底）** —— 每 N 毫秒无论是否持续输入都保存一次，
//!    覆盖"连续高速输入导致防抖永不触发"的极端场景。
//!
//! ## 写入安全
//! 复用 [`EditorState::save_file`] 的原子写入路径（临时文件 + fsync + rename），
//! 任何时刻崩溃要么保留旧文件、要么得到完整新文件。
//!
//! ## 内容去重
//! 比对 `buffer_version` 与 `last_saved_buffer_version`，相同则跳过写盘，
//! 避免无意义 IO（输入又删回、撤销回原状态等）。
//!
//! ## 冲突检测
//! 保存前比对磁盘 mtime 与 `last_known_mtime`，外部修改则暂停自动保存并提示，
//! 不静默覆盖。
//!
//! ## 大文件降级
//! 文件体积超过阈值时延长防抖、关闭周期保存，仅保留失焦保存，避免 IO 卡顿。

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use crate::editor::EditorState;

/// 防抖定时器 ID（空闲保存）。0xA005，紧接现有 0xA004。
pub(crate) const AUTOSAVE_DEBOUNCE_TIMER_ID: usize = 0xA005;
/// 周期兜底定时器 ID。0xA006。
pub(crate) const AUTOSAVE_PERIODIC_TIMER_ID: usize = 0xA006;

/// 自动保存触发原因
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoSaveReason {
    /// 空闲防抖触发
    Debounce,
    /// 失焦触发
    FocusLoss,
    /// 周期兜底触发
    Periodic,
}

impl EditorState {
    /// 当前缓冲区是否属于大文件（依据字节数与配置阈值）
    pub fn is_large_file_for_autosave(&self) -> bool {
        let threshold = self.app_settings.auto_save.large_file_threshold;
        threshold > 0 && (self.content.buffer.len_bytes() as u64) > threshold
    }

    /// 计算当前文件应使用的防抖延迟（大文件降级为更长延迟）
    pub fn effective_autosave_debounce_ms(&self) -> u32 {
        let cfg = &self.app_settings.auto_save;
        if self.is_large_file_for_autosave() {
            cfg.large_file_debounce_ms
        } else {
            cfg.debounce_ms
        }
    }

    /// 在每次文本编辑后调用：按防抖延迟（重）设防抖定时器。
    ///
    /// `SetTimer` 对同 ID 定时器会重置计时，天然实现防抖。
    /// 仅对有路径（可落盘）且脏的文件调度。
    pub fn schedule_autosave_debounce(&self) {
        let cfg = &self.app_settings.auto_save;
        if !cfg.enabled {
            return;
        }
        if self.content.file_path.is_none() || !self.content.is_dirty {
            return;
        }
        let delay = self.effective_autosave_debounce_ms();
        if delay == 0 {
            return;
        }
        unsafe {
            // SetTimer 对已存在的同 ID 定时器重置计时
            let _ = SetTimer(self.hwnd, AUTOSAVE_DEBOUNCE_TIMER_ID, delay, None);
        }
    }

    /// 停止防抖定时器（保存成功后或文件变为干净时调用）
    pub fn stop_autosave_debounce(&self) {
        unsafe {
            let _ = KillTimer(self.hwnd, AUTOSAVE_DEBOUNCE_TIMER_ID);
        }
    }

    /// 启动周期兜底定时器（在窗口创建后调用一次）
    pub fn start_autosave_periodic(&self) {
        let cfg = &self.app_settings.auto_save;
        if !cfg.enabled || cfg.periodic_save_ms == 0 {
            return;
        }
        unsafe {
            let _ = SetTimer(
                self.hwnd,
                AUTOSAVE_PERIODIC_TIMER_ID,
                cfg.periodic_save_ms,
                None,
            );
        }
    }

    /// 失焦触发：编辑器失去焦点时立即保存当前标签
    pub fn autosave_on_focus_loss(&mut self) {
        let enabled = self.app_settings.auto_save.enabled;
        let focus_loss_save = self.app_settings.auto_save.focus_loss_save;
        if !enabled || !focus_loss_save {
            return;
        }
        self.autosave_tick(AutoSaveReason::FocusLoss);
    }

    /// 防抖定时器触发：WM_TIMER(AUTOSAVE_DEBOUNCE_TIMER_ID)
    pub fn on_autosave_debounce_timer(&mut self) {
        // 防抖触发后停止定时器，下一次编辑会重新调度
        self.stop_autosave_debounce();
        self.autosave_tick(AutoSaveReason::Debounce);
    }

    /// 周期定时器触发：WM_TIMER(AUTOSAVE_PERIODIC_TIMER_ID)
    pub fn on_autosave_periodic_timer(&mut self) {
        let cfg = &self.app_settings.auto_save;
        if !cfg.enabled || cfg.periodic_save_ms == 0 {
            return;
        }
        // 大文件降级：关闭周期保存，仅保留失焦与防抖
        if self.is_large_file_for_autosave() {
            return;
        }
        self.autosave_tick(AutoSaveReason::Periodic);
    }

    /// 执行一次自动保存尝试（含去重、冲突检测、落盘）。
    ///
    /// 保存逻辑复用 [`EditorState::save_file`]，与手动 Ctrl+S 走完全相同的
    /// 原子写入路径，保证行为一致性。
    pub fn autosave_tick(&mut self, reason: AutoSaveReason) {
        let enabled = self.app_settings.auto_save.enabled;
        if !enabled {
            return;
        }
        // 无路径文件无法自动保存（需用户另存为）
        let path = match self.content.file_path.clone() {
            Some(p) => p,
            None => return,
        };
        // 远程文件走远程保存路径，不参与本地自动保存
        if path.to_str().map_or(false, |s| s.starts_with("remote:")) {
            return;
        }
        // 未修改：跳过
        if !self.content.is_dirty {
            return;
        }
        // 内容去重：buffer_version 未变则跳过写盘
        if self.content.buffer_version == self.content.last_saved_buffer_version {
            return;
        }
        // 冲突检测：mtime 变化则暂停自动保存并提示
        if self.detect_autosave_conflict(&path) {
            return;
        }
        let _ = reason; // 目前所有触发原因走相同路径；保留枚举便于后续差异化处理

        // 实际落盘：复用 save_file 的原子写入路径
        if self.save_file() {
            // save_file 已将 is_dirty=false 并更新 last_saved_buffer_version/mtime/conflict
            // 静默提示（不打扰）：状态栏轻量确认
            self.status_message = "已自动保存".to_string();
        }
        // 保存失败：save_file 已设置错误 status_message；保留内容不清空，等待下次重试
    }

    /// 保存成功后同步自动保存状态（手动 Ctrl+S / 另存为 / 自动保存共用）。
    ///
    /// - 对齐 `last_saved_buffer_version`，使后续去重正确跳过
    /// - 复位 `auto_save_conflict`（手动保存视为用户确认覆盖外部修改）
    /// - 刷新 `last_known_mtime` 为落盘后的新 mtime（仅本地文件）
    /// - 停止防抖定时器（已落盘，无需再防抖）
    pub(crate) fn note_save_succeeded(&mut self) {
        self.content.last_saved_buffer_version = self.content.buffer_version;
        self.content.auto_save_conflict = false;
        self.content.last_known_mtime = self.content.file_path.as_ref().and_then(|p| {
            // 仅本地文件有 mtime；远程文件（remote: 前缀）跳过
            if p.to_str().map_or(false, |s| s.starts_with("remote:")) {
                None
            } else {
                std::fs::metadata(p).and_then(|m| m.modified()).ok()
            }
        });
        self.stop_autosave_debounce();
    }

    /// 检测外部修改：比对当前磁盘 mtime 与上次已知 mtime。
    ///
    /// 返回 `true` 表示检测到冲突（应暂停自动保存）。
    /// 手动保存（Ctrl+S）会复位 `auto_save_conflict` 并更新基线 mtime。
    fn detect_autosave_conflict(&mut self, path: &std::path::Path) -> bool {
        let current_mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return false, // 文件不可读，不在此处阻断（save 时会报错）
        };
        match self.content.last_known_mtime {
            Some(known) if current_mtime > known => {
                if !self.content.auto_save_conflict {
                    self.content.auto_save_conflict = true;
                    self.status_message =
                        "文件已被外部修改，自动保存已暂停。按 Ctrl+S 覆盖或重新载入".to_string();
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_shared::settings::AutoSaveSettings;

    #[test]
    fn autosave_default_config_matches_design_doc() {
        // 设计文档"推荐默认配置"验证
        let cfg = AutoSaveSettings::default();
        assert!(cfg.enabled, "总开关默认开启");
        assert_eq!(cfg.debounce_ms, 1000, "空闲防抖 1000ms");
        assert!(cfg.focus_loss_save, "失焦保存开启");
        assert_eq!(cfg.periodic_save_ms, 30_000, "兜底周期 30s");
        assert_eq!(cfg.large_file_threshold, 2 * 1024 * 1024, "大文件阈值 2MB");
        assert_eq!(cfg.large_file_debounce_ms, 5000, "大文件防抖 5s");
    }

    #[test]
    fn autosave_settings_deserialize_with_missing_field_uses_defaults() {
        // 旧版 settings.json 不含 auto_save 字段时，应回退到默认值
        let json = r#"{}"#;
        let cfg: AutoSaveSettings = serde_json::from_str(json).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.debounce_ms, 1000);
        assert_eq!(cfg.periodic_save_ms, 30_000);
    }

    #[test]
    fn autosave_settings_deserialize_partial_override() {
        let json = r#"{"enabled": false, "debounce_ms": 500}"#;
        let cfg: AutoSaveSettings = serde_json::from_str(json).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.debounce_ms, 500);
        // 未指定的字段仍用默认
        assert!(cfg.focus_loss_save);
        assert_eq!(cfg.periodic_save_ms, 30_000);
    }

    #[test]
    fn timer_ids_are_distinct_and_in_range() {
        assert_eq!(AUTOSAVE_DEBOUNCE_TIMER_ID, 0xA005);
        assert_eq!(AUTOSAVE_PERIODIC_TIMER_ID, 0xA006);
        assert_ne!(AUTOSAVE_DEBOUNCE_TIMER_ID, AUTOSAVE_PERIODIC_TIMER_ID);
    }
}
