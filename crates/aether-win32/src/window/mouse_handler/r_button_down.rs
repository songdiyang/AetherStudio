//! `WM_RBUTTONDOWN` / `WM_RBUTTONUP` 处理：标签 + 资源管理器空白区域上下文菜单。
//!
//! 标签右键：当用户在标签栏的某个标签上右键点击时弹出标签上下文菜单。
//! 资源管理器空白区域：当用户在侧边栏文件树的空白区域（未命中任何文件/文件夹节点，
//! 也未命中标题栏的新建按钮）右键点击时，弹出上下文菜单。
//! 在其他区域右键点击时，关闭已打开的上下文菜单。

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use super::super::{get_and_set_state, invalidate_window};
use crate::activity_bar_context_menu::ActivityBarContextMenuState;
use crate::editor::EditorState;
use crate::layout::SidebarContent;
use crate::tab_context_menu::TabContextMenuState;

/// 文件树渲染起始 y 偏移（与 handle_file_tree_click 中的 34.0 保持一致：
/// header_h=28 + 分隔线 + 少量上边距）
const FILE_TREE_LIST_START_Y: f32 = 34.0;

/// WM_RBUTTONDOWN
pub(crate) unsafe fn on_r_button_down(
    hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let raw_x = (lparam.0 & 0xFFFF) as i16 as f32;
    let raw_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let Some(state) = get_and_set_state(hwnd) else {
        return LRESULT(0);
    };
    let (mouse_x, mouse_y, window_w, window_h) = {
        let st = state.borrow();
        (raw_x / st.dpi_scale, raw_y / st.dpi_scale, st.window_width as f32, st.window_height as f32)
    };

    let mut st = state.borrow_mut();

    // 若正在内联输入，右键先取消输入（与左键逻辑一致）
    if st.file_tree_input.is_some() {
        st.cancel_file_tree_input();
    }

    // SubTask 9.3: 标签右键——检测是否命中标签栏的某个标签
    let show_tab_bar = st.show_tab_bar();
    let tab_region = st.layout.tab_bar_region(show_tab_bar);
    if show_tab_bar && tab_region.contains(mouse_x, mouse_y) {
        if let Some(tab_idx) = st.tab_body_hit_test(mouse_x, mouse_y, tab_region.x) {
            // 获取该标签的 file_path（用于判断 has_path 和复制路径）
            let has_path = st
                .tabs
                .get(tab_idx)
                .and_then(|t| t.content.file_path.as_ref())
                .is_some();
            let mut menu = TabContextMenuState::build_for_tab(tab_idx, has_path);
            menu.open_at(mouse_x, mouse_y, window_w, window_h);
            st.tab_context_menu = menu;
            // 关闭可能打开的资源管理器菜单，避免重叠
            if st.explorer_context_menu.is_open {
                st.explorer_context_menu.close();
            }
            // 菜单互斥：关闭活动栏菜单
            if st.activity_bar_context_menu.visible {
                st.activity_bar_context_menu.hide();
            }
            drop(st);
            invalidate_window(hwnd);
            return LRESULT(0);
        }
    }

    // SubTask 14.1: 活动栏右键——检测是否落在活动栏区域
    let activity_region = st.layout.activity_bar_region();
    if st.layout.activity_bar_visible
        && activity_region.width > 0.0
        && activity_region.contains(mouse_x, mouse_y)
    {
        let active_view = st.activity_view;
        let mut menu = ActivityBarContextMenuState::build(active_view);
        menu.open_at(mouse_x, mouse_y, window_w, window_h);
        st.activity_bar_context_menu = menu;
        // 菜单互斥：关闭标签菜单与资源管理器菜单
        if st.tab_context_menu.visible {
            st.tab_context_menu.hide();
        }
        if st.explorer_context_menu.is_open {
            st.explorer_context_menu.close();
        }
        drop(st);
        invalidate_window(hwnd);
        return LRESULT(0);
    }

    // 仅当侧边栏可见且当前为文件树视图时，才可能弹出空白区域菜单
    let sidebar_region = st.layout.sidebar_region();
    let in_sidebar_file_tree = st.layout.sidebar_visible
        && sidebar_region.width > 0.0
        && sidebar_region.contains(mouse_x, mouse_y)
        && st.sidebar_content == SidebarContent::FileTree;

    if !in_sidebar_file_tree {
        // 在侧边栏外右键：关闭已打开的菜单
        let mut need_invalidate = false;
        if st.explorer_context_menu.is_open {
            st.explorer_context_menu.close();
            need_invalidate = true;
        }
        if st.tab_context_menu.visible {
            st.tab_context_menu.hide();
            need_invalidate = true;
        }
        if st.activity_bar_context_menu.visible {
            st.activity_bar_context_menu.hide();
            need_invalidate = true;
        }
        drop(st);
        if need_invalidate {
            invalidate_window(hwnd);
        }
        return LRESULT(0);
    }

    // 命中标题栏的新建按钮 → 不弹出空白菜单（交由左键处理）
    let on_new_file_btn = st
        .file_tree_new_file_btn
        .as_ref()
        .map(|r| r.contains(mouse_x, mouse_y))
        .unwrap_or(false);
    let on_new_folder_btn = st
        .file_tree_new_folder_btn
        .as_ref()
        .map(|r| r.contains(mouse_x, mouse_y))
        .unwrap_or(false);
    if on_new_file_btn || on_new_folder_btn {
        return LRESULT(0);
    }

    // 侧边栏内坐标（相对侧边栏左上角）
    let sidebar_rel_x = mouse_x - sidebar_region.x;
    let sidebar_rel_y = mouse_y - sidebar_region.y;
    let sidebar_width = st.layout.sidebar_width;

    // 命中文件/文件夹节点 → 选中该节点但不弹出空白区域菜单
    // （节点级上下文菜单不在本次需求范围内）
    // 先以不可变借用查询命中节点，提取 node_idx 后再修改状态，避免借用冲突。
    let hit_node_idx: Option<u32> = st.file_tree.as_ref().and_then(|tree| {
        let mut current_y = FILE_TREE_LIST_START_Y;
        EditorState::find_tree_click_target(
            tree,
            u32::MAX,
            sidebar_rel_x,
            sidebar_rel_y,
            sidebar_width,
            &mut current_y,
        )
        .map(|(node_idx, _, _)| node_idx)
    });

    if let Some(node_idx) = hit_node_idx {
        // 节点命中：选中节点，关闭旧菜单
        let mut need_invalidate = false;
        if st.explorer_context_menu.is_open {
            st.explorer_context_menu.close();
            need_invalidate = true;
        }
        if st.tab_context_menu.visible {
            st.tab_context_menu.hide();
            need_invalidate = true;
        }
        if st.activity_bar_context_menu.visible {
            st.activity_bar_context_menu.hide();
            need_invalidate = true;
        }
        st.selected_file_node = Some(node_idx);
        st.emit_event(crate::events::EditorEvent::SidebarChanged);
        drop(st);
        if need_invalidate {
            invalidate_window(hwnd);
        }
        return LRESULT(0);
    }

    // 空白区域：弹出上下文菜单（菜单内部会做窗口边界校正）
    st.explorer_context_menu
        .open(mouse_x, mouse_y, window_w, window_h);
    // 关闭可能打开的标签菜单，避免重叠
    if st.tab_context_menu.visible {
        st.tab_context_menu.hide();
    }
    if st.activity_bar_context_menu.visible {
        st.activity_bar_context_menu.hide();
    }
    // 空白区域右键同时清除当前选中节点（符合"未选中任何文件或文件夹"语义）
    st.selected_file_node = None;
    st.emit_event(crate::events::EditorEvent::SidebarChanged);
    drop(st);
    invalidate_window(hwnd);
    LRESULT(0)
}

/// WM_RBUTTONUP
///
/// 当前上下文菜单由 WM_LBUTTONDOWN 处理点击，右键抬起无需做事。
/// 保留空实现以阻止 DefWindowProc 弹出系统默认菜单。
pub(crate) unsafe fn on_r_button_up(
    _hwnd: HWND,
    _msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    LRESULT(0)
}
