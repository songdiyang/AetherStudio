use super::*;

impl EditorState {
    /// 通知 LSP 服务器文档已打开（按需启动 server）。
    /// 在 load_file 后调用，激活智能补全/悬停/诊断。
    /// 异步执行：克隆所需数据后 spawn tokio task，不阻塞 UI 线程。
    ///
    /// 重要：get_all_text 是 O(N) 全文件 String 拷贝，对大文件耗时明显。
    /// 之前在 UI 线程上调用（line 1690），导致打开第一个文件时严重卡顿。
    /// 修复：把 buffer Arc 克隆进 spawn，由后台线程读取并构造 LSP 文本。
    pub(super) fn lsp_notify_open(&self) {
        // 1. 映射语言到 LSP language_id（无配置则跳过）
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };

        // 2. 转换文件路径到 Url（LSP 要求 file:// URI）
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };

        // 3. 克隆 Arc<buffer>，把昂贵的 get_all_text 推迟到后台线程
        let buffer = self.content.buffer.clone();
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            // 后台线程读取全文件（O(N) 拷贝，不再阻塞 UI 渲染）
            let text = buffer.get_all_text();
            // 按需启动 server：如果未就绪且有默认配置，启动它
            if !client.is_server_ready(&lang_id).await {
                let config = match default_server_config(&lang_id) {
                    Some(c) => c,
                    None => return, // 该语言没有默认 server 配置，跳过
                };
                if let Err(e) = client.start_server(&lang_id, config).await {
                    eprintln!("LSP start_server({}) failed: {}", lang_id, e);
                    return;
                }
            }
            // 发送 did_open
            if let Err(e) = client.open_document(uri, lang_id, text).await {
                eprintln!("LSP open_document failed: {}", e);
            }
        });
    }
    /// 通知 LSP 服务器文档内容已变更。
    /// 在 insert_char/delete_char/insert_newline/delete_forward 后调用。
    pub(super) fn lsp_notify_change(&self) {
        // 1. 映射语言到 LSP language_id（无配置则跳过）
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };

        // 2. 转换文件路径到 Url
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };

        // 3. 全文档同步：直接发送全文，由 LspClient 内部计算增量变更
        let text = self.content.buffer.get_all_text();

        // 4. Spawn 异步任务发送 did_change
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            // 注意：notify_change 内部会检查 document_sync 是否有该文档，
            // 如果 did_open 尚未完成，change 会被静默丢弃。这在实践中可接受：
            // 用户在 server 启动后的第一次编辑会正常同步。
            if let Err(e) = client.notify_change(&uri, &text).await {
                eprintln!("LSP notify_change({}) failed: {}", lang_id, e);
            }
        });
    }
    /// Phase H1: 请求补全（Ctrl+Space 触发）。
    /// 异步调用 LSP request_completion，结果通过 LspEvent::Completion 回传。
    pub(crate) fn request_completion(&mut self) {
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };
        // LSP Position：line 0-based，character 为 UTF-16 偏移（ASCII 下等同字节列）
        let position = lsp_types::Position {
            line: self.content.cursor_line as u32,
            character: self.content.cursor_col as u32,
        };
        // 记录触发位置，用于弹窗定位
        self.completion_trigger_line = self.content.cursor_line;
        self.completion_trigger_col = self.content.cursor_col;

        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            if !client.is_server_ready(&lang_id).await {
                return; // server 未就绪，静默跳过
            }
            if let Err(e) = client.request_completion(&uri, position).await {
                eprintln!("LSP request_completion({}) failed: {}", lang_id, e);
            }
        });
    }
    /// Phase H3: 请求悬停信息（鼠标悬停触发）。
    /// 异步调用 LSP request_hover，结果通过 LspEvent::Hover 回传。
    /// 注意：触发逻辑（WM_MOUSEMOVE + 定时器去抖）尚未接线，此方法预留就绪。
    #[allow(dead_code)]
    pub(crate) fn request_hover(&mut self, line: usize, col: usize) {
        let language_id = match language_to_lsp_id_opt(self.content.language) {
            Some(id) => id.to_string(),
            None => return,
        };
        let path = match &self.content.file_path {
            Some(p) => p.as_path(),
            None => return,
        };
        let uri = match Url::from_file_path(path) {
            Ok(u) => u,
            Err(_) => return,
        };
        let position = lsp_types::Position {
            line: line as u32,
            character: col as u32,
        };
        let client = self.lsp_client.clone();
        let lang_id = language_id;
        self.tokio_runtime.spawn(async move {
            if !client.is_server_ready(&lang_id).await {
                return;
            }
            if let Err(e) = client.request_hover(&uri, position).await {
                eprintln!("LSP request_hover({}) failed: {}", lang_id, e);
            }
        });
    }
    /// 处理从 LSP 服务器收到的异步事件（由 WM_APP+3 调用）。
    /// 第一版重点处理 Diagnostics（诊断更新）和 ServerReady（状态提示）。
    /// 其他事件（Completion/Hover/References 等）留待 Phase H 接线 UI 组件。
    pub(crate) fn handle_lsp_event(&mut self, event: LspEvent) {
        match event {
            LspEvent::Diagnostics { uri, diagnostics } => {
                // 更新诊断表：按 uri 存储，UI 渲染时查询当前文件
                let count = diagnostics.len();
                if diagnostics.is_empty() {
                    self.lsp_diagnostics.remove(&uri);
                } else {
                    self.lsp_diagnostics.insert(uri, diagnostics);
                }
                // 状态栏提示诊断数量（仅当前文件）
                if let Some(path) = &self.content.file_path {
                    if let Ok(current_uri) = Url::from_file_path(path) {
                        let current_count = self
                            .lsp_diagnostics
                            .get(&current_uri)
                            .map(|v| v.len())
                            .unwrap_or(0);
                        if current_count > 0 {
                            self.status_message = format!("诊断: {} 个问题", current_count);
                        } else {
                            self.status_message = "无诊断问题".to_string();
                        }
                    }
                }
                let _ = count; // 避免 unused 警告
            }
            LspEvent::ServerReady { language_id } => {
                self.status_message = format!("LSP 服务器就绪: {}", language_id);
            }
            LspEvent::Log {
                language_id,
                message,
            } => {
                // 服务器日志输出到 stderr，不显示在状态栏（避免刷屏）
                eprintln!("[LSP/{}] {}", language_id, message);
            }
            // Phase H1: 补全结果到达，显示弹窗
            LspEvent::Completion { uri, items } => {
                if items.is_empty() {
                    self.completion_visible = false;
                } else {
                    // 验证是当前文件的补全结果
                    let is_current = self
                        .content
                        .file_path
                        .as_ref()
                        .and_then(|p| Url::from_file_path(p).ok())
                        .map(|u| u == uri)
                        .unwrap_or(false);
                    if is_current {
                        self.completion_items = items;
                        self.completion_selected = 0;
                        self.completion_visible = true;
                    }
                }
            }
            // Phase H3: 悬停结果到达，显示 tooltip
            LspEvent::Hover { uri, hover } => {
                let is_current = self
                    .content
                    .file_path
                    .as_ref()
                    .and_then(|p| Url::from_file_path(p).ok())
                    .map(|u| u == uri)
                    .unwrap_or(false);
                if is_current {
                    self.hover_content = extract_hover_text(&hover);
                }
            }
            // 未接线的 LSP 事件静默忽略
            LspEvent::References { .. }
            | LspEvent::Rename { .. }
            | LspEvent::CodeActions { .. }
            | LspEvent::Formatting { .. }
            | LspEvent::SemanticTokens { .. }
            | LspEvent::SemanticTokensDelta { .. }
            | LspEvent::InlayHints { .. } => {
                // 后续版本接线
            }
        }
    }

    // ===== Phase H2: 补全弹窗导航 =====
    /// 补全列表下一项（↓ 键）
    pub(crate) fn completion_next(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        self.completion_selected = (self.completion_selected + 1) % self.completion_items.len();
    }
    /// 补全列表上一项（↑ 键）
    pub(crate) fn completion_prev(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        if self.completion_selected == 0 {
            self.completion_selected = self.completion_items.len() - 1;
        } else {
            self.completion_selected -= 1;
        }
    }
    /// 接受当前选中的补全项（Enter 键）。
    /// 将光标移回触发位置，删除已输入的过滤文本，插入补全项的 insert_text 或 label。
    pub(crate) fn completion_accept(&mut self) {
        if !self.completion_visible || self.completion_items.is_empty() {
            return;
        }
        let item = self.completion_items[self.completion_selected].clone();
        // 优先用 insert_text，其次 label
        let insert_text = item
            .insert_text
            .clone()
            .unwrap_or_else(|| item.label.clone());
        // 关闭弹窗
        self.completion_visible = false;
        self.completion_items.clear();
        // 将光标移回触发位置
        if self.content.cursor_line != self.completion_trigger_line {
            return; // 跨行编辑，放弃插入
        }
        // 删除触发位置到当前光标之间的文本（用户输入的过滤字符）
        let delete_count = self
            .content
            .cursor_col
            .saturating_sub(self.completion_trigger_col);
        for _ in 0..delete_count {
            self.delete_char();
        }
        // 插入补全文本
        for ch in insert_text.chars() {
            self.insert_char(ch);
        }
    }
    /// 关闭补全弹窗（Esc 键 或 失焦）
    pub(crate) fn completion_cancel(&mut self) {
        if self.completion_visible {
            self.completion_visible = false;
            self.completion_items.clear();
        }
    }
    /// 关闭悬停 tooltip
    #[allow(dead_code)]
    pub(crate) fn hover_cancel(&mut self) {
        self.hover_content = None;
    }
    /// 初始化 LSP 客户端（在打开工作区文件夹时调用）
    pub fn init_lsp(&mut self, root_dir: &Path) {
        // 如果已有 LSP 客户端，先清理
        self.legacy_lsp_client = None;
        self.lsp_rx = None;
        self.lsp_runtime = None;

        let root_uri = url::Url::from_directory_path(root_dir).ok();
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };

        let (client, rx) = aether_lsp::client::LspClient::new(root_uri.clone());

        // 在 tokio 运行时中启动 Rust 语言服务器（如果 rust-analyzer 可用）
        if let Some(config) = aether_lsp::client::default_server_config("rust") {
            let config = aether_lsp::types::ServerConfig {
                root_uri: root_uri.clone(),
                ..config
            };
            let client_clone = std::sync::Arc::new(client);
            let client_for_spawn = std::sync::Arc::clone(&client_clone);
            runtime.spawn(async move {
                let _ = client_for_spawn.start_server("rust", config).await;
            });
            self.legacy_lsp_client = Some(client_clone);
        } else {
            self.legacy_lsp_client = Some(std::sync::Arc::new(client));
        }

        self.lsp_rx = Some(rx);
        self.lsp_runtime = Some(runtime);
    }
    /// 通知 LSP 服务器文档已打开
    pub fn lsp_open_document(&mut self, path: &Path, text: &str) {
        let Some(client) = self.legacy_lsp_client.clone() else {
            return;
        };
        let Some(runtime) = self.lsp_runtime.as_ref() else {
            return;
        };
        let Ok(uri) = url::Url::from_file_path(path) else {
            return;
        };
        let language_id = language_to_lsp_id(self.content.language).to_string();
        let text = text.to_string();
        runtime.spawn(async move {
            let _ = client.open_document(uri, language_id, text).await;
        });
    }
    /// 轮询 LSP 事件，将诊断同步到 LspClient 缓存和 EditorState.diagnostics
    /// 应在渲染循环中每帧调用
    pub fn poll_lsp_events(&mut self) {
        let Some(rx) = self.lsp_rx.as_mut() else {
            return;
        };
        while let Ok(event) = rx.try_recv() {
            use aether_lsp::client::LspEvent;
            match event {
                LspEvent::Diagnostics { uri, diagnostics } => {
                    // 同时写入 LspClient 诊断缓存（供其他模块查询）
                    if let Some(client) = self.legacy_lsp_client.as_ref() {
                        client.update_diagnostics(&uri, diagnostics.clone());
                    }

                    let path_str = match uri.to_file_path() {
                        Ok(p) => p.to_string_lossy().to_string(),
                        Err(()) => uri.as_str().to_string(),
                    };
                    let items: Vec<DiagnosticItem> = diagnostics
                        .iter()
                        .map(|d| {
                            use aether_lsp::lsp_types::DiagnosticSeverity;
                            let severity = match d.severity {
                                Some(DiagnosticSeverity::ERROR) => 1,
                                Some(DiagnosticSeverity::WARNING) => 2,
                                Some(DiagnosticSeverity::INFORMATION) => 3,
                                Some(DiagnosticSeverity::HINT) => 4,
                                _ => 1,
                            };
                            DiagnosticItem {
                                severity,
                                message: d.message.clone(),
                                line: d.range.start.line as usize + 1,
                                col: d.range.start.character as usize + 1,
                                end_line: d.range.end.line as usize + 1,
                                end_col: d.range.end.character as usize + 1,
                            }
                        })
                        .collect();
                    if items.is_empty() {
                        self.diagnostics.remove(&path_str);
                    } else {
                        self.diagnostics.insert(path_str, items);
                    }
                }
                LspEvent::ServerReady { language_id } => {
                    self.status_message = format!("LSP 服务器就绪: {}", language_id);
                }
                LspEvent::Log { message, .. } => {
                    tracing::debug!("LSP: {}", message);
                }
                _ => {}
            }
        }
    }
}
