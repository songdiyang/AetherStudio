use std::path::PathBuf;

use aether_remote::ssh::{SshAuth, SshConfig, SshRemoteFs};
use aether_remote::{RemoteDirEntry, RemoteFs};

/// SSH 认证类型（UI 层）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshAuthType {
    Password,
    Key,
    Agent,
}

/// 对话框操作结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogAction {
    None,
    Connect,
    Cancel,
}

/// SSH 连接对话框状态
#[derive(Clone, Debug)]
pub struct SshConnectionDialog {
    pub visible: bool,
    pub host: String,
    pub port: String,
    pub username: String,
    pub auth_type: SshAuthType,
    pub password: String,
    pub key_path: String,
    pub key_passphrase: String,
    pub error_message: Option<String>,
    /// 当前焦点字段索引 (0=host, 1=port, 2=username, 3=password/keypath, 4=passphrase)
    pub focus_field: usize,
    /// 按钮悬停状态 (0=connect, 1=cancel)
    pub hover_button: Option<usize>,
    /// 连接按钮区域（渲染时更新，用于点击检测）
    pub connect_btn_rect: Option<crate::layout::Region>,
    /// 取消按钮区域（渲染时更新，用于点击检测）
    pub cancel_btn_rect: Option<crate::layout::Region>,
}

impl SshConnectionDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            host: String::new(),
            port: "22".to_string(),
            username: String::new(),
            auth_type: SshAuthType::Password,
            password: String::new(),
            key_path: String::new(),
            key_passphrase: String::new(),
            error_message: None,
            focus_field: 0,
            hover_button: None,
            connect_btn_rect: None,
            cancel_btn_rect: None,
        }
    }

    pub fn reset(&mut self) {
        self.host.clear();
        self.port = "22".to_string();
        self.username.clear();
        self.auth_type = SshAuthType::Password;
        self.password.clear();
        self.key_path.clear();
        self.key_passphrase.clear();
        self.error_message = None;
        self.focus_field = 0;
        self.hover_button = None;
        self.connect_btn_rect = None;
        self.cancel_btn_rect = None;
    }

    pub fn to_config(&self) -> Option<SshConfig> {
        if self.host.is_empty() || self.username.is_empty() {
            return None;
        }
        let port = self.port.parse().ok().unwrap_or(22);
        let auth = match self.auth_type {
            SshAuthType::Password => SshAuth::Password(self.password.clone()),
            SshAuthType::Key => SshAuth::Key {
                path: self.key_path.clone(),
                passphrase: if self.key_passphrase.is_empty() {
                    None
                } else {
                    Some(self.key_passphrase.clone())
                },
            },
            SshAuthType::Agent => SshAuth::Agent,
        };

        Some(SshConfig {
            host: self.host.clone(),
            port,
            username: self.username.clone(),
            auth,
        })
    }

    /// 切换到下一下焦点字段
    pub fn next_field(&mut self) {
        let max_field = match self.auth_type {
            SshAuthType::Password => 3,
            SshAuthType::Key => 4,
            SshAuthType::Agent => 2,
        };
        self.focus_field = (self.focus_field + 1) % (max_field + 1);
    }
}

/// 远程会话状态
pub struct RemoteSession {
    pub config: SshConfig,
    pub fs: SshRemoteFs,
    pub connected: bool,
    pub current_path: String,
    pub error_message: Option<String>,
}

impl RemoteSession {
    pub fn new(config: SshConfig) -> Self {
        let fs = SshRemoteFs::new(config.clone());
        Self {
            config,
            fs,
            connected: false,
            current_path: "/".to_string(),
            error_message: None,
        }
    }

    pub fn connect(&mut self) -> Result<(), String> {
        self.fs.connect().map_err(|e| e.to_string())?;
        self.connected = true;
        self.error_message = None;
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.fs.disconnect();
        self.connected = false;
    }

    pub fn is_connected(&self) -> bool {
        self.connected && self.fs.is_connected()
    }

    /// 列出当前路径下的文件
    pub fn list_current_dir(&self) -> Result<Vec<RemoteDirEntry>, String> {
        self.fs
            .list_dir(&self.current_path)
            .map_err(|e| e.to_string())
    }

    /// 读取远程文件
    pub fn read_remote_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.fs.read_file(path).map_err(|e| e.to_string())
    }

    /// 写入远程文件
    pub fn write_remote_file(&self, path: &str, content: &[u8]) -> Result<(), String> {
        self.fs.write_file(path, content).map_err(|e| e.to_string())
    }

    /// 执行远程命令
    pub fn exec(&self, command: &str) -> Result<(String, String), String> {
        self.fs.exec(command).map_err(|e| e.to_string())
    }
}

/// 远程文件树节点
#[derive(Clone, Debug)]
pub struct RemoteFileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub depth: u8,
    pub children: Vec<RemoteFileNode>,
}

/// 远程文件树
#[derive(Clone, Debug)]
pub struct RemoteFileTree {
    pub nodes: Vec<RemoteFileNode>,
    pub root_path: String,
}

impl RemoteFileTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            root_path: "/".to_string(),
        }
    }

    pub fn from_entries(path: &str, entries: Vec<RemoteDirEntry>) -> Self {
        let mut nodes = Vec::new();
        for entry in entries {
            let node_path = if path == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", path, entry.name)
            };
            nodes.push(RemoteFileNode {
                name: entry.name.clone(),
                path: node_path,
                is_dir: entry.is_dir,
                is_expanded: false,
                depth: 0,
                children: Vec::new(),
            });
        }
        // 排序：目录在前，文件在后，按名称排序
        nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        Self {
            nodes,
            root_path: path.to_string(),
        }
    }
}

/// 克隆仓库对话框
#[derive(Clone, Debug)]
pub struct CloneRepoDialog {
    pub visible: bool,
    pub url: String,
    pub target_path: Option<PathBuf>,
    pub error_message: Option<String>,
    pub focus_field: usize,
    pub hover_button: Option<usize>,
    pub clone_btn_rect: Option<crate::layout::Region>,
    pub cancel_btn_rect: Option<crate::layout::Region>,
}

impl CloneRepoDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            url: String::new(),
            target_path: None,
            error_message: None,
            focus_field: 0,
            hover_button: None,
            clone_btn_rect: None,
            cancel_btn_rect: None,
        }
    }

    pub fn reset(&mut self) {
        self.url.clear();
        self.target_path = None;
        self.error_message = None;
        self.focus_field = 0;
        self.hover_button = None;
        self.clone_btn_rect = None;
        self.cancel_btn_rect = None;
    }
}
