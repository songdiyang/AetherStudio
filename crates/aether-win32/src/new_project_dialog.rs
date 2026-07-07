use std::path::PathBuf;

/// 新建项目对话框点击结果
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NewProjectDialogAction {
    Confirm,
    Cancel,
    FocusInput,
    None,
}

/// 新建项目对话框状态
#[derive(Clone, Debug)]
pub struct NewProjectDialog {
    pub visible: bool,
    /// 用户输入的项目名称
    pub project_name: String,
    /// 默认项目根目录（如 C:\Users\<user>\Documents\AetherProjects）
    pub base_path: PathBuf,
    /// 错误提示
    pub error_message: Option<String>,
    /// 焦点字段 (0=项目名称输入框)
    pub focus_field: usize,
    /// 悬停的按钮 (0=确认, 1=取消)
    pub hover_button: Option<usize>,
    /// 输入框区域（渲染时更新，用于点击检测）
    pub input_rect: Option<crate::layout::Region>,
    /// 确认按钮区域
    pub confirm_btn_rect: Option<crate::layout::Region>,
    /// 取消按钮区域
    pub cancel_btn_rect: Option<crate::layout::Region>,
    /// 光标是否可见（用于闪烁效果）
    pub caret_visible: bool,
}

impl NewProjectDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            project_name: String::new(),
            base_path: Self::default_base_path(),
            error_message: None,
            focus_field: 0,
            hover_button: None,
            input_rect: None,
            confirm_btn_rect: None,
            cancel_btn_rect: None,
            caret_visible: true,
        }
    }

    pub fn reset(&mut self) {
        self.project_name.clear();
        self.error_message = None;
        self.focus_field = 0;
        self.hover_button = None;
        self.input_rect = None;
        self.confirm_btn_rect = None;
        self.cancel_btn_rect = None;
        self.caret_visible = true;
        // 每次打开对话框时刷新默认路径，避免旧路径失效或被移动
        self.base_path = Self::default_base_path();
    }

    /// 计算默认项目根目录：使用本地应用数据目录，避免文档目录权限问题
    fn default_base_path() -> PathBuf {
        dirs::data_local_dir()
            .map(|d| d.join("Aether").join("Projects"))
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("AetherProjects")
            })
    }

    /// 获取完整项目路径
    pub fn project_path(&self) -> PathBuf {
        self.base_path.join(&self.project_name)
    }

    /// 验证项目名称是否合法
    pub fn validate(&self) -> Result<(), String> {
        let name = self.project_name.trim();
        if name.is_empty() {
            return Err("请输入项目名称".to_string());
        }
        if name
            .chars()
            .any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '\"' | '<' | '>' | '|'))
        {
            return Err("项目名称包含非法字符".to_string());
        }
        let path = self.project_path();
        if path.exists() {
            return Err("该项目已存在".to_string());
        }
        Ok(())
    }
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_project_dialog_defaults() {
        let dialog = NewProjectDialog::new();
        assert!(!dialog.visible);
        assert!(dialog.project_name.is_empty());
        assert!(dialog.error_message.is_none());
        assert_eq!(dialog.focus_field, 0);
        assert!(!dialog.base_path.as_os_str().is_empty());
    }

    #[test]
    fn test_project_path() {
        let mut dialog = NewProjectDialog::new();
        dialog.base_path = PathBuf::from("D:\\Projects");
        dialog.project_name = "MyApp".to_string();
        assert_eq!(dialog.project_path(), PathBuf::from("D:\\Projects\\MyApp"));
    }

    #[test]
    fn test_validate_empty_name() {
        let mut dialog = NewProjectDialog::new();
        dialog.project_name = "   ".to_string();
        assert_eq!(dialog.validate(), Err("请输入项目名称".to_string()));
    }

    #[test]
    fn test_validate_invalid_chars() {
        let mut dialog = NewProjectDialog::new();
        dialog.base_path = PathBuf::from("D:\\Projects");
        dialog.project_name = "a/b".to_string();
        assert_eq!(dialog.validate(), Err("项目名称包含非法字符".to_string()));
    }

    #[test]
    fn test_validate_existing_path() {
        let mut dialog = NewProjectDialog::new();
        dialog.base_path = std::env::temp_dir();
        dialog.project_name = std::process::id().to_string();
        // 先创建目录使其已存在
        let path = dialog.project_path();
        std::fs::create_dir_all(&path).unwrap();
        let result = dialog.validate();
        let _ = std::fs::remove_dir_all(&path);
        assert_eq!(result, Err("该项目已存在".to_string()));
    }

    #[test]
    fn test_validate_valid_name() {
        let mut dialog = NewProjectDialog::new();
        dialog.base_path = std::env::temp_dir();
        dialog.project_name = format!("valid_project_{}", std::process::id());
        assert!(dialog.validate().is_ok());
    }

    #[test]
    fn test_reset_clears_name() {
        let mut dialog = NewProjectDialog::new();
        dialog.project_name = "test".to_string();
        dialog.error_message = Some("error".to_string());
        dialog.reset();
        assert!(dialog.project_name.is_empty());
        assert!(dialog.error_message.is_none());
    }
}
