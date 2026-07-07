// new_without_default: 多数 UI 组件的 new() 包含主题初始化数据，不适合 Default
#![allow(clippy::new_without_default)]

pub mod diff_view;
pub mod dirty_rect;
pub mod events;
pub mod focus_manager;
pub mod launch;
pub mod render_context;

pub mod activity_bar;
pub mod ai_agent;
pub mod ai_context;
pub mod ai_panel;
pub mod ai_prompt;
pub mod command_palette;
pub mod dialogs;
pub mod editor;
pub mod git;
pub mod hit_test;
pub mod icons;
pub mod ime;
pub mod inline_completion;
pub mod input;
pub mod layout;
pub mod logging;
pub mod menu_bar;
pub mod new_project_dialog;
pub mod open_tabs;
pub mod recent_projects;
pub mod render;
pub mod search_panel;
pub mod settings;
pub mod ssh;
pub mod status_bar;
pub mod tabs;
pub mod terminal;
pub mod theme;
pub mod uia;
pub mod user_menu;
pub mod welcome;
pub mod window;
