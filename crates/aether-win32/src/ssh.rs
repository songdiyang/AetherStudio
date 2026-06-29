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
            // P1-2: 默认 Agent 认证（密码认证已禁用）
            auth_type: SshAuthType::Agent,
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
        // P1-2: 默认 Agent 认证（密码认证已禁用）
        self.auth_type = SshAuthType::Agent;
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
        // P1-2: 密码认证已禁用——即便 UI 残留 Password 选项，此处也不再生成
        // Password 认证，回退为 Agent（纵深防御，connect 层另有一道拦截）
        let auth = match self.auth_type {
            SshAuthType::Password => SshAuth::Agent,
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

    /// P0-1: 列出指定路径下的目录内容（用于子目录懒加载）
    pub fn list_dir(&self, path: &str) -> Result<Vec<RemoteDirEntry>, String> {
        self.fs.list_dir(path).map_err(|e| e.to_string())
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
    /// P0-1: 子节点是否已加载（区分"未加载"与"空目录"）
    pub children_loaded: bool,
    /// P0-1: 子节点正在异步加载中（显示 loading 指示，防止重复触发）
    pub is_loading: bool,
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
                children_loaded: false,
                is_loading: false,
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

    /// P0-1: 从 RemoteDirEntry 列表构造子节点（内部辅助）
    fn build_children(path: &str, entries: Vec<RemoteDirEntry>, depth: u8) -> Vec<RemoteFileNode> {
        let mut children = Vec::new();
        for entry in entries {
            let node_path = if path == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", path, entry.name)
            };
            children.push(RemoteFileNode {
                name: entry.name.clone(),
                path: node_path,
                is_dir: entry.is_dir,
                is_expanded: false,
                depth,
                children: Vec::new(),
                children_loaded: false,
                is_loading: false,
            });
        }
        // 排序：目录在前，文件在后，按名称排序
        children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        children
    }

    /// P0-1: 递归查找节点（按路径）
    pub fn find_node(&self, path: &str) -> Option<&RemoteFileNode> {
        for node in &self.nodes {
            if node.path == path {
                return Some(node);
            }
            if node.is_expanded {
                if let Some(found) = Self::find_node_in_children(node, path) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_node_in_children<'a>(
        node: &'a RemoteFileNode,
        path: &str,
    ) -> Option<&'a RemoteFileNode> {
        for child in &node.children {
            if child.path == path {
                return Some(child);
            }
            if child.is_expanded {
                if let Some(found) = Self::find_node_in_children(child, path) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// P0-1: 递归查找节点（可变引用，按路径）
    pub fn find_node_mut(&mut self, path: &str) -> Option<&mut RemoteFileNode> {
        for node in &mut self.nodes {
            if node.path == path {
                return Some(node);
            }
            if node.is_expanded {
                if let Some(found) = Self::find_node_mut_in_children(node, path) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_node_mut_in_children<'a>(
        node: &'a mut RemoteFileNode,
        path: &str,
    ) -> Option<&'a mut RemoteFileNode> {
        for child in &mut node.children {
            if child.path == path {
                return Some(child);
            }
            if child.is_expanded {
                if let Some(found) = Self::find_node_mut_in_children(child, path) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// P0-1: 展开节点——填充子节点并标记为已加载
    ///
    /// 找到 path 对应的目录节点，将 entries 填入 children，置 children_loaded=true、
    /// is_expanded=true、is_loading=false。未找到节点则无操作。
    pub fn expand_node(&mut self, path: &str, entries: Vec<RemoteDirEntry>) {
        if let Some(node) = self.find_node_mut(path) {
            if !node.is_dir {
                return;
            }
            let depth = node.depth + 1;
            node.children = Self::build_children(path, entries, depth);
            node.children_loaded = true;
            node.is_expanded = true;
            node.is_loading = false;
        }
    }

    /// P0-1: 标记节点加载失败（清除 loading 状态）
    pub fn mark_node_load_failed(&mut self, path: &str) {
        if let Some(node) = self.find_node_mut(path) {
            node.is_loading = false;
        }
    }

    /// P0-1: 统计当前可见节点数（用于滚动高度估算）
    ///
    /// 顶层节点始终可见；展开目录的子节点才参与计数。
    pub fn count_visible_nodes(&self) -> usize {
        let mut count = 0;
        for node in &self.nodes {
            count += 1;
            if node.is_expanded {
                count += Self::count_visible_children(node);
            }
        }
        count
    }

    fn count_visible_children(node: &RemoteFileNode) -> usize {
        let mut count = 0;
        for child in &node.children {
            count += 1;
            if child.is_expanded {
                count += Self::count_visible_children(child);
            }
        }
        count
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

/// SSH 管理面板状态（侧边栏）
/// 管理服务器列表的增删改查、连接状态显示
#[derive(Clone, Debug)]
pub struct SshManagerPanel {
    /// 是否处于添加/编辑模式
    pub editing: bool,
    /// 编辑模式下正在编辑的服务器索引（None=新增）
    pub edit_index: Option<usize>,
    // --- 编辑表单字段 ---
    pub form_name: String,
    pub form_host: String,
    pub form_port: String,
    pub form_username: String,
    pub form_auth_type: SshAuthType,
    pub form_key_path: String,
    /// 焦点字段 (0=name, 1=host, 2=port, 3=username, 4=key_path)
    pub focus_field: usize,
    /// 错误消息
    pub error_message: Option<String>,
    // --- 列表交互 ---
    /// 当前选中的服务器索引
    pub selected: Option<usize>,
    /// 悬停的服务器索引
    pub hover: Option<usize>,
    /// 悬停的操作按钮类型 (0=connect, 1=edit, 2=delete)
    pub hover_action: Option<(usize, usize)>,
    /// 滚动偏移
    pub scroll_y: f32,
    // --- 按钮区域（渲染时更新，用于点击检测） ---
    pub add_btn_rect: Option<crate::layout::Region>,
    pub save_btn_rect: Option<crate::layout::Region>,
    pub cancel_btn_rect: Option<crate::layout::Region>,
    /// 每个服务器条目的按钮区域: (index, action) -> Region
    pub item_btn_rects: Vec<(usize, usize, crate::layout::Region)>,
}

impl SshManagerPanel {
    pub fn new() -> Self {
        Self {
            editing: false,
            edit_index: None,
            form_name: String::new(),
            form_host: String::new(),
            form_port: "22".to_string(),
            form_username: String::new(),
            form_auth_type: SshAuthType::Agent,
            form_key_path: String::new(),
            focus_field: 0,
            error_message: None,
            selected: None,
            hover: None,
            hover_action: None,
            scroll_y: 0.0,
            add_btn_rect: None,
            save_btn_rect: None,
            cancel_btn_rect: None,
            item_btn_rects: Vec::new(),
        }
    }

    /// 开始添加新服务器
    pub fn start_add(&mut self) {
        self.editing = true;
        self.edit_index = None;
        self.form_name.clear();
        self.form_host.clear();
        self.form_port = "22".to_string();
        self.form_username.clear();
        self.form_auth_type = SshAuthType::Agent;
        self.form_key_path.clear();
        self.focus_field = 0;
        self.error_message = None;
    }

    /// 开始编辑已有服务器
    pub fn start_edit(&mut self, index: usize, config: &aether_shared::settings::SshServerConfig) {
        self.editing = true;
        self.edit_index = Some(index);
        self.form_name = config.name.clone();
        self.form_host = config.host.clone();
        self.form_port = config.port.to_string();
        self.form_username = config.username.clone();
        self.form_auth_type = config.auth_type.as_str().into();
        self.form_key_path = config.key_path.clone();
        self.focus_field = 0;
        self.error_message = None;
    }

    /// 取消编辑
    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_index = None;
        self.error_message = None;
    }

    /// P1-2: 循环切换认证方式（密码认证已禁用，仅 Agent ↔ Key）
    pub fn cycle_auth_type(&mut self) {
        self.form_auth_type = match self.form_auth_type {
            SshAuthType::Agent => SshAuthType::Key,
            // Password 已禁用：Key 之后直接回到 Agent，跳过 Password
            SshAuthType::Key => SshAuthType::Agent,
            // 兜底：任何其他状态（含遗留 Password）回退到 Agent
            SshAuthType::Password => SshAuthType::Agent,
        };
    }

    /// 将表单字段转换为持久化配置
    pub fn form_to_config(&self) -> Result<aether_shared::settings::SshServerConfig, String> {
        if self.form_name.trim().is_empty() {
            return Err("请输入服务器名称".to_string());
        }
        if self.form_host.trim().is_empty() {
            return Err("请输入主机地址".to_string());
        }
        if self.form_username.trim().is_empty() {
            return Err("请输入用户名".to_string());
        }
        // P1-2: 密码认证禁用——表单层拦截，不允许保存 password 配置
        if self.form_auth_type == SshAuthType::Password {
            return Err(
                "密码认证不支持（shell out 模式无 tty 无法交互输入密码），请使用密钥或 Agent 认证"
                    .to_string(),
            );
        }
        let port: u16 = self
            .form_port
            .parse()
            .map_err(|_| "端口号无效".to_string())?;
        let auth_type = match self.form_auth_type {
            SshAuthType::Password => "password", // 理论不可达（上方已拦截），保留 exhaustiveness
            SshAuthType::Key => "key",
            SshAuthType::Agent => "agent",
        };
        // P1-2: 密钥认证必须提供 key_path
        if self.form_auth_type == SshAuthType::Key && self.form_key_path.trim().is_empty() {
            return Err("密钥认证需要指定密钥文件路径".to_string());
        }
        Ok(aether_shared::settings::SshServerConfig {
            name: self.form_name.trim().to_string(),
            host: self.form_host.trim().to_string(),
            port,
            username: self.form_username.trim().to_string(),
            auth_type: auth_type.to_string(),
            key_path: if self.form_auth_type == SshAuthType::Key {
                self.form_key_path.clone()
            } else {
                String::new()
            },
        })
    }

    /// 将持久化配置转换为可连接的 SshConfig
    ///
    /// P1-2: 密码认证已禁用。若配置中残留 "password"（旧版本保存），
    /// 此处作为纵深防御回退为 Agent 认证，确保不会生成 Password 变体。
    pub fn config_to_ssh_config(
        config: &aether_shared::settings::SshServerConfig,
    ) -> aether_remote::ssh::SshConfig {
        let auth = match config.auth_type.as_str() {
            "key" => aether_remote::ssh::SshAuth::Key {
                path: config.key_path.clone(),
                passphrase: None,
            },
            // P1-2: password 不再生成 Password 认证，回退为 Agent（纵深防御）
            "password" | _ => aether_remote::ssh::SshAuth::Agent,
        };
        aether_remote::ssh::SshConfig {
            host: config.host.clone(),
            port: config.port,
            username: config.username.clone(),
            auth,
        }
    }
}

impl From<&str> for SshAuthType {
    fn from(s: &str) -> Self {
        match s {
            "password" => SshAuthType::Password,
            "key" => SshAuthType::Key,
            _ => SshAuthType::Agent,
        }
    }
}
