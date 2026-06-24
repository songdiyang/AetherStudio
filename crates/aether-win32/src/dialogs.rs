use std::path::PathBuf;

use windows::core::GUID;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows::Win32::UI::Shell::{
    IFileOpenDialog, IFileSaveDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

const CLSID_FILEOPENDIALOG: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
const CLSID_FILESAVEDIALOG: GUID = GUID::from_u128(0xC0B4E2F3_BA21_4773_8DBA_335EC946EB8B);

/// 文件对话框
pub struct Dialogs;

impl Dialogs {
    /// 打开文件夹对话框
    pub fn open_folder_dialog(hwnd: HWND, title: &str) -> Option<PathBuf> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

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

    /// 打开文件对话框
    pub fn open_file_dialog(hwnd: HWND, title: &str, _filters: &[(&str, &str)]) -> Option<PathBuf> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

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

    /// 保存文件对话框
    pub fn save_file_dialog(hwnd: HWND, title: &str, default_name: &str) -> Option<PathBuf> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

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
