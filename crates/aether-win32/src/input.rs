use std::collections::HashMap;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_LEFT,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};

/// 按键类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8), // F1-F12
    Ctrl,
    Shift,
    Alt,
}

/// 快捷键定义
#[derive(Clone, Debug)]
pub struct KeyBinding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub action: EditorAction,
}

/// 编辑器动作
#[derive(Clone, Debug, PartialEq)]
pub enum EditorAction {
    // 文件操作
    OpenFile,
    OpenFolder,
    Save,
    SaveAll,
    CloseTab,
    Exit,

    // 编辑操作
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    Replace,

    // 视图操作
    ToggleSidebar,
    ToggleTerminal,
    ToggleAiPanel,
    ZoomIn,
    ZoomOut,

    // 光标移动
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
    MoveFileStart,
    MoveFileEnd,

    // 选择
    SelectUp,
    SelectDown,
    SelectLeft,
    SelectRight,
    SelectWord,
    SelectLine,

    // 多光标
    AddCursorAbove,
    AddCursorBelow,
    AddCursorAtNextOccurrence,

    // 代码操作
    ToggleComment,
    FormatDocument,

    // 其他
    NewFile,
    ToggleFullScreen,
    ShowCommandPalette,
    TriggerAi,
}

/// 键盘映射
pub struct KeyMap {
    bindings: HashMap<(Key, bool, bool, bool), EditorAction>,
}

impl KeyMap {
    pub fn new() -> Self {
        let mut bindings = HashMap::new();

        // 文件操作
        bindings.insert((Key::Char('o'), true, false, false), EditorAction::OpenFile);
        bindings.insert(
            (Key::Char('k'), true, false, false),
            EditorAction::OpenFolder,
        );
        bindings.insert((Key::Char('s'), true, false, false), EditorAction::Save);
        bindings.insert((Key::Char('s'), true, true, false), EditorAction::SaveAll);
        bindings.insert((Key::Char('w'), true, false, false), EditorAction::CloseTab);
        bindings.insert((Key::Char('n'), true, false, false), EditorAction::NewFile);

        // 编辑操作
        bindings.insert((Key::Char('z'), true, false, false), EditorAction::Undo);
        bindings.insert((Key::Char('z'), true, true, false), EditorAction::Redo);
        bindings.insert((Key::Char('x'), true, false, false), EditorAction::Cut);
        bindings.insert((Key::Char('c'), true, false, false), EditorAction::Copy);
        bindings.insert((Key::Char('v'), true, false, false), EditorAction::Paste);
        bindings.insert(
            (Key::Char('a'), true, false, false),
            EditorAction::SelectAll,
        );
        bindings.insert((Key::Char('f'), true, false, false), EditorAction::Find);
        bindings.insert((Key::Char('h'), true, false, false), EditorAction::Replace);

        // 视图操作
        bindings.insert(
            (Key::Char('b'), true, false, false),
            EditorAction::ToggleSidebar,
        );
        bindings.insert(
            (Key::Char('`'), true, false, false),
            EditorAction::ToggleTerminal,
        );
        bindings.insert(
            (Key::Char(' '), true, false, false),
            EditorAction::ShowCommandPalette,
        );
        bindings.insert((Key::Char('='), true, false, false), EditorAction::ZoomIn);
        bindings.insert((Key::Char('-'), true, false, false), EditorAction::ZoomOut);

        // 代码操作
        bindings.insert(
            (Key::Char('/'), true, false, false),
            EditorAction::ToggleComment,
        );
        bindings.insert(
            (Key::Char('d'), true, true, false),
            EditorAction::FormatDocument,
        );

        // 多光标
        bindings.insert(
            (Key::ArrowUp, true, true, false),
            EditorAction::AddCursorAbove,
        );
        bindings.insert(
            (Key::ArrowDown, true, true, false),
            EditorAction::AddCursorBelow,
        );
        bindings.insert(
            (Key::Char('d'), true, false, false),
            EditorAction::AddCursorAtNextOccurrence,
        );

        // AI
        bindings.insert((Key::Space, true, false, false), EditorAction::TriggerAi);

        Self { bindings }
    }

    /// 查找按键对应的动作
    pub fn lookup(&self, key: Key, ctrl: bool, shift: bool, alt: bool) -> Option<&EditorAction> {
        self.bindings.get(&(key, ctrl, shift, alt))
    }

    /// 从Win32虚拟键码转换
    pub fn from_vk(
        vk: VIRTUAL_KEY,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<(Key, bool, bool, bool)> {
        let key = match vk {
            VK_RETURN => Key::Enter,
            VK_TAB => Key::Tab,
            VK_BACK => Key::Backspace,
            VK_DELETE => Key::Delete,
            VK_ESCAPE => Key::Escape,
            VK_SPACE => Key::Space,
            VK_UP => Key::ArrowUp,
            VK_DOWN => Key::ArrowDown,
            VK_LEFT => Key::ArrowLeft,
            VK_RIGHT => Key::ArrowRight,
            VK_HOME => Key::Home,
            VK_END => Key::End,
            VK_PRIOR => Key::PageUp,
            VK_NEXT => Key::PageDown,
            _ if vk.0 >= VK_F1.0 && vk.0 <= VK_F12.0 => Key::F((vk.0 - VK_F1.0 + 1) as u8),
            _ => {
                if let Some(ch) = vk_to_char(vk, shift) {
                    Key::Char(ch)
                } else {
                    return None;
                }
            }
        };

        Some((key, ctrl, shift, alt))
    }
}

/// 将虚拟键码转换为字符（简化）
fn vk_to_char(vk: VIRTUAL_KEY, shift: bool) -> Option<char> {
    let vk_code = vk.0;
    match vk_code {
        0x30..=0x39 => {
            let ch = (b'0' + (vk_code - 0x30) as u8) as char;
            Some(ch)
        }
        0x41..=0x5A => {
            let ch = if shift {
                (b'A' + (vk_code - 0x41) as u8) as char
            } else {
                (b'a' + (vk_code - 0x41) as u8) as char
            };
            Some(ch)
        }
        0xBA => Some(if shift { ':' } else { ';' }),
        0xBB => Some(if shift { '+' } else { '=' }),
        0xBC => Some(if shift { '<' } else { ',' }),
        0xBD => Some(if shift { '_' } else { '-' }),
        0xBE => Some(if shift { '>' } else { '.' }),
        0xBF => Some(if shift { '?' } else { '/' }),
        0xC0 => Some(if shift { '~' } else { '`' }),
        0xDB => Some(if shift { '{' } else { '[' }),
        0xDC => Some(if shift { '|' } else { '\\' }),
        0xDD => Some(if shift { '}' } else { ']' }),
        0xDE => Some(if shift { '"' } else { '\'' }),
        _ => None,
    }
}

/// 多光标管理器
pub struct MultiCursor {
    cursors: Vec<Cursor>,
    primary: usize,
}

#[derive(Clone, Debug)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
}

impl MultiCursor {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor {
                line: 0,
                col: 0,
                selection_start: None,
                selection_end: None,
            }],
            primary: 0,
        }
    }

    pub fn add_cursor(&mut self, line: usize, col: usize) {
        self.cursors.push(Cursor {
            line,
            col,
            selection_start: None,
            selection_end: None,
        });
    }

    pub fn remove_secondary(&mut self) {
        if self.cursors.len() > 1 {
            self.cursors.truncate(1);
            self.primary = 0;
        }
    }

    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    pub fn cursors_mut(&mut self) -> &mut [Cursor] {
        &mut self.cursors
    }

    pub fn primary(&self) -> &Cursor {
        &self.cursors[self.primary]
    }

    pub fn primary_mut(&mut self) -> &mut Cursor {
        &mut self.cursors[self.primary]
    }
}

impl Default for MultiCursor {
    fn default() -> Self {
        Self::new()
    }
}
