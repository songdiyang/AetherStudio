/// 编辑器事件总线
///
/// 目标：
/// - 将模型层改动与渲染层解耦
/// - 批量收集一帧内的事件，合并同类事件，避免重复重绘
/// - 为后续 ViewModel / Viewport 渲染提供统一事件源
use crate::dirty_rect::{DirtyRectTracker, DirtyRegionType};

/// 编辑器事件枚举
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    /// 文本内容变化（插入/删除）
    TextChanged {
        start_line: usize,
        end_line: usize,
    },
    /// 光标移动
    CursorMoved,
    /// 选择变化
    SelectionChanged,
    /// 滚动偏移变化
    Scrolled,
    /// 标签页切换/新增/关闭
    TabChanged,
    /// 侧边栏内容变化（文件树、Git 状态等）
    SidebarChanged,
    /// 右侧面板变化（AI 面板等）
    RightPanelChanged,
    /// 底部面板变化（终端输出等）
    BottomPanelChanged,
    /// 状态栏变化
    StatusBarChanged,
    /// 窗口尺寸变化 / DPI 变化
    WindowResized,
    /// 查找替换面板状态变化
    FindReplaceChanged,
    /// 对话框显示/隐藏
    DialogVisibilityChanged,
}

impl EditorEvent {
    /// 事件对应的脏区域类型
    pub fn region_type(&self) -> DirtyRegionType {
        match self {
            EditorEvent::TextChanged { .. }
            | EditorEvent::CursorMoved
            | EditorEvent::SelectionChanged
            | EditorEvent::Scrolled => DirtyRegionType::EditorContent,
            EditorEvent::TabChanged => DirtyRegionType::TabBar,
            EditorEvent::SidebarChanged => DirtyRegionType::Sidebar,
            EditorEvent::RightPanelChanged => DirtyRegionType::RightPanel,
            EditorEvent::BottomPanelChanged => DirtyRegionType::BottomPanel,
            EditorEvent::StatusBarChanged => DirtyRegionType::StatusBar,
            EditorEvent::WindowResized => DirtyRegionType::FullWindow,
            EditorEvent::FindReplaceChanged => DirtyRegionType::FindReplace,
            EditorEvent::DialogVisibilityChanged => DirtyRegionType::FullWindow,
        }
    }

    /// 是否需要全窗口重绘
    pub fn is_full_window(&self) -> bool {
        matches!(
            self,
            EditorEvent::WindowResized | EditorEvent::DialogVisibilityChanged
        )
    }
}

/// 事件队列
///
/// 收集一帧内的事件，在渲染前统一 drain 并应用到 DirtyRectTracker。
#[derive(Clone, Debug, Default)]
pub struct EventQueue {
    events: Vec<EditorEvent>,
    /// 是否有全窗口重绘事件（一旦标记，后续局部事件可忽略）
    has_full_window: bool,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(16),
            has_full_window: false,
        }
    }

    /// 入队一个事件；自动合并/降级
    pub fn push(&mut self, event: EditorEvent) {
        if event.is_full_window() {
            self.has_full_window = true;
            // 清空已有局部事件，因为全窗口会覆盖它们
            self.events.clear();
            self.events.push(event);
            return;
        }

        if self.has_full_window {
            // 已有全窗口事件，忽略局部事件
            return;
        }

        // 尝试合并同类事件
        if let Some(last) = self.events.last_mut() {
            if Self::try_merge(last, &event) {
                return;
            }
        }

        self.events.push(event);
    }

    /// 批量入队
    pub fn extend(&mut self, events: impl IntoIterator<Item = EditorEvent>) {
        for event in events {
            self.push(event);
        }
    }

    /// 尝试合并两个事件；成功返回 true
    fn try_merge(existing: &mut EditorEvent, incoming: &EditorEvent) -> bool {
        match (existing, incoming) {
            // 连续滚动合并为一次滚动
            (EditorEvent::Scrolled, EditorEvent::Scrolled) => true,
            // 连续光标移动合并为一次
            (EditorEvent::CursorMoved, EditorEvent::CursorMoved) => true,
            // 选择变化合并为一次
            (EditorEvent::SelectionChanged, EditorEvent::SelectionChanged) => true,
            // 文本变化合并行范围
            (
                EditorEvent::TextChanged {
                    start_line: s1,
                    end_line: e1,
                },
                EditorEvent::TextChanged {
                    start_line: s2,
                    end_line: e2,
                },
            ) => {
                *s1 = (*s1).min(*s2);
                *e1 = (*e1).max(*e2);
                true
            }
            // 其他不合并
            _ => false,
        }
    }

    /// 将队列中事件应用到脏矩形追踪器
    ///
    /// 调用方需提供把事件转换为矩形尺寸的回调。
    pub fn drain_to_dirty_tracker<F>(
        &mut self,
        tracker: &mut DirtyRectTracker,
        mut event_to_rect: F,
    ) where
        F: FnMut(EditorEvent) -> Option<(f32, f32, f32, f32)>,
    {
        for event in self.events.drain(..) {
            if event.is_full_window() {
                tracker.mark_full_window();
                continue;
            }
            if let Some((x, y, w, h)) = event_to_rect(event) {
                tracker.mark_region(x, y, w, h, event.region_type());
            }
        }
        self.has_full_window = false;
    }

    /// 清空队列
    pub fn clear(&mut self) {
        self.events.clear();
        self.has_full_window = false;
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        !self.has_full_window && self.events.is_empty()
    }

    /// 事件数量（调试用）
    pub fn len(&self) -> usize {
        if self.has_full_window {
            1
        } else {
            self.events.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_scrolled_events() {
        let mut q = EventQueue::new();
        q.push(EditorEvent::Scrolled);
        q.push(EditorEvent::Scrolled);
        q.push(EditorEvent::Scrolled);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn full_window_clears_local_events() {
        let mut q = EventQueue::new();
        q.push(EditorEvent::CursorMoved);
        q.push(EditorEvent::Scrolled);
        q.push(EditorEvent::WindowResized);
        q.push(EditorEvent::CursorMoved);
        assert_eq!(q.len(), 1);
        assert!(q.has_full_window);
    }

    #[test]
    fn merge_text_changed_line_range() {
        let mut q = EventQueue::new();
        q.push(EditorEvent::TextChanged {
            start_line: 5,
            end_line: 7,
        });
        q.push(EditorEvent::TextChanged {
            start_line: 3,
            end_line: 9,
        });
        assert_eq!(q.len(), 1);
        if let EditorEvent::TextChanged { start_line, end_line } = q.events[0] {
            assert_eq!(start_line, 3);
            assert_eq!(end_line, 9);
        } else {
            panic!("expected TextChanged");
        }
    }
}
