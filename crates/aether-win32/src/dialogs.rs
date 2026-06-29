use std::path::PathBuf;

use windows::core::GUID;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows::Win32::UI::Shell::{
    IFileOpenDialog, IFileSaveDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// UI-M08: COM 初始化 RAII 守卫，确保 CoInitializeEx 与 CoUninitialize 配对
struct ComGuard {
    needs_uninit: bool,
}

impl ComGuard {
    fn init() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            // S_OK = COM 成功初始化, S_FALSE = 已初始化（不负责释放）
            Self {
                needs_uninit: hr.is_ok(),
            }
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.needs_uninit {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

const CLSID_FILEOPENDIALOG: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
const CLSID_FILESAVEDIALOG: GUID = GUID::from_u128(0xC0B4E2F3_BA21_4773_8DBA_335EC946EB8B);

/// 文件对话框
pub struct Dialogs;

impl Dialogs {
    /// 打开文件夹对话框
    pub fn open_folder_dialog(hwnd: HWND, title: &str) -> Option<PathBuf> {
        // UI-M08: 使用 ComGuard RAII 确保 CoUninitialize 被调用
        let _com = ComGuard::init();
        unsafe {
            let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
                &CLSID_FILEOPENDIALOG,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .ok()?;

            // 设置选项：选择文件夹
            let mut options = dialog.GetOptions().ok()?;
            options |= FOS_PICKFOLDERS;
            dialog.SetOptions(options).ok()?;

            // 设置标题
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            dialog
                .SetTitle(windows::core::PCWSTR(title_wide.as_ptr()))
                .ok()?;

            // 设置默认起始目录为上次打开位置
            if let Some(last_dir) = last_folder::get() {
                if let Ok(shell_item) =
                    windows::Win32::UI::Shell::SHCreateItemFromParsingName::<
                        _,
                        _,
                        windows::Win32::UI::Shell::IShellItem,
                    >(windows::core::PCWSTR(last_dir.as_ptr()), None)
                {
                    let _ = dialog.SetDefaultFolder(&shell_item);
                    let _ = dialog.SetFolder(&shell_item);
                }
            }

            // 显示对话框
            if dialog.Show(hwnd).is_err() {
                return None;
            }

            // 获取结果
            let result = dialog.GetResult().ok()?;
            let path_ptr = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = path_ptr.to_string().ok()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(path_ptr.0 as *const _));

            let path_buf = PathBuf::from(&path);
            // 记忆此次打开的目录
            last_folder::set(&path_buf);
            Some(path_buf)
        }
    }

    /// 打开文件对话框
    pub fn open_file_dialog(hwnd: HWND, title: &str, _filters: &[(&str, &str)]) -> Option<PathBuf> {
        let _com = ComGuard::init();
        unsafe {
            let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
                &CLSID_FILEOPENDIALOG,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .ok()?;

            // 设置标题
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            dialog
                .SetTitle(windows::core::PCWSTR(title_wide.as_ptr()))
                .ok()?;

            // 设置默认起始目录
            if let Some(last_dir) = last_folder::get() {
                if let Ok(shell_item) =
                    windows::Win32::UI::Shell::SHCreateItemFromParsingName::<
                        _,
                        _,
                        windows::Win32::UI::Shell::IShellItem,
                    >(windows::core::PCWSTR(last_dir.as_ptr()), None)
                {
                    let _ = dialog.SetDefaultFolder(&shell_item);
                    let _ = dialog.SetFolder(&shell_item);
                }
            }

            // 显示对话框
            if dialog.Show(hwnd).is_err() {
                return None;
            }

            // 获取结果
            let result = dialog.GetResult().ok()?;
            let path_ptr = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = path_ptr.to_string().ok()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(path_ptr.0 as *const _));

            let path_buf = PathBuf::from(&path);
            // 记忆此次打开的目录（取父目录）
            if let Some(parent) = path_buf.parent() {
                last_folder::set(parent);
            }
            Some(path_buf)
        }
    }

    /// 保存文件对话框
    pub fn save_file_dialog(hwnd: HWND, title: &str, default_name: &str) -> Option<PathBuf> {
        let _com = ComGuard::init();
        unsafe {
            let dialog: IFileSaveDialog = windows::Win32::System::Com::CoCreateInstance(
                &CLSID_FILESAVEDIALOG,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .ok()?;

            // 设置标题
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            dialog
                .SetTitle(windows::core::PCWSTR(title_wide.as_ptr()))
                .ok()?;

            // 设置默认文件名
            let name_wide: Vec<u16> = default_name.encode_utf16().chain(Some(0)).collect();
            dialog
                .SetFileName(windows::core::PCWSTR(name_wide.as_ptr()))
                .ok()?;

            // 显示对话框
            if dialog.Show(hwnd).is_err() {
                return None;
            }

            // 获取结果
            let result = dialog.GetResult().ok()?;
            let path_ptr = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = path_ptr.to_string().ok()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(path_ptr.0 as *const _));

            Some(PathBuf::from(path))
        }
    }

    /// 打开C文件对话框（快捷方法）
    pub fn open_c_file(hwnd: HWND) -> Option<PathBuf> {
        Self::open_file_dialog(
            hwnd,
            "打开C文件",
            &[("C源文件", "*.c"), ("C头文件", "*.h"), ("所有文件", "*.*")],
        )
    }

    /// 显示错误对话框（模态）
    pub fn show_error(hwnd: HWND, title: &str, message: &str) {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let msg_wide: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let _ = MessageBoxW(
                hwnd,
                windows::core::PCWSTR(msg_wide.as_ptr()),
                windows::core::PCWSTR(title_wide.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    /// 显示"是/否"确认对话框（模态）。返回 true 表示用户选"是"。
    pub fn confirm_yes_no(hwnd: HWND, title: &str, message: &str) -> bool {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO,
            };
            let title_wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let msg_wide: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let result = MessageBoxW(
                hwnd,
                windows::core::PCWSTR(msg_wide.as_ptr()),
                windows::core::PCWSTR(title_wide.as_ptr()),
                MB_YESNO | MB_ICONQUESTION,
            );
            result == IDYES
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialogs() {
        // 对话框测试需要GUI环境，这里仅验证结构
        let _dialogs = Dialogs;
    }
}

/// 上次打开的文件夹持久化（与 recent_projects 共用 APPDATA/Aether 目录）
mod last_folder {
    use std::fs;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    const FILE_NAME: &str = "last_folder.txt";

    fn config_dir() -> PathBuf {
        let app_data = std::env::var("APPDATA")
            .or_else(|_| std::env::var("HOME"))
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(app_data).join("Aether")
    }

    fn file_path() -> PathBuf {
        config_dir().join(FILE_NAME)
    }

    /// 读取上次打开的文件夹路径，返回 UTF-16 编码的宽字符串（含结尾 \0）
    pub fn get() -> Option<Vec<u16>> {
        let path = file_path();
        let mut buf = String::new();
        fs::File::open(&path).ok()?.read_to_string(&mut buf).ok()?;
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.encode_utf16().chain(Some(0)).collect())
    }

    /// 写入上次打开的文件夹路径
    pub fn set(path: &Path) {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        if let Ok(mut file) = fs::File::create(file_path()) {
            let _ = file.write_all(path.to_string_lossy().as_bytes());
        }
    }
}

/// 已信任的工作区目录持久化（防止重复弹窗）
pub mod trusted_folders {
    use std::collections::HashSet;
    use std::fs;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    const FILE_NAME: &str = "trusted_folders.txt";

    fn config_dir() -> PathBuf {
        let app_data = std::env::var("APPDATA")
            .or_else(|_| std::env::var("HOME"))
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(app_data).join("Aether");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn file_path() -> PathBuf {
        config_dir().join(FILE_NAME)
    }

    /// 加载已信任目录集合
    fn load_all() -> HashSet<String> {
        let mut set = HashSet::new();
        if let Ok(mut file) = fs::File::open(file_path()) {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok() {
                for line in contents.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        set.insert(trimmed.to_string());
                    }
                }
            }
        }
        set
    }

    /// 判断目录是否已信任
    pub fn is_trusted(path: &Path) -> bool {
        let key = path.to_string_lossy().to_lowercase();
        load_all().contains(&key)
    }

    /// 将目录加入信任列表
    pub fn add_trusted(path: &Path) {
        let key = path.to_string_lossy().to_lowercase();
        let mut all = load_all();
        all.insert(key);
        if let Ok(mut file) = fs::File::create(file_path()) {
            for p in &all {
                let _ = file.write_all(p.as_bytes());
                let _ = file.write_all(b"\n");
            }
        }
    }
}
