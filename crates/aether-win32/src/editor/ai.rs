use super::*;

impl EditorState {
    /// 复制最后一条 AI 回复到剪贴板
    pub fn copy_ai_last_response(&mut self) {
        if let Some(t) = self.ai_panel.last_assistant_text() {
            if Self::set_clipboard_text(&t) {
                self.status_message = "已复制 AI 回复".to_string();
            }
        }
    }
    /// 保存 AI 代码块为文件
    /// 如果 filename 为空，则尝试从代码块内容推断或使用默认名称
    pub fn save_ai_code_block(
        &mut self,
        code: &str,
        suggested_filename: Option<&str>,
    ) -> std::result::Result<PathBuf, String> {
        let root = self
            .current_folder
            .clone()
            .ok_or_else(|| "请先打开一个工作区文件夹".to_string())?;

        // 确定文件名
        let filename = if let Some(name) = suggested_filename {
            name.to_string()
        } else {
            // 尝试从代码内容推断语言并生成默认文件名
            let ext = if code.contains("fn ") || code.contains("use ") || code.contains("impl ") {
                "rs"
            } else if code.contains("def ") || code.contains("import ") {
                "py"
            } else if code.contains("function ") || code.contains("const ") || code.contains("let ")
            {
                "js"
            } else if code.contains("package ") || code.contains("import java.") {
                "java"
            } else if code.contains("#include") || code.contains("int main") {
                "c"
            } else if code.contains("<?php") {
                "php"
            } else if code.contains("<html") || code.contains("<!DOCTYPE") {
                "html"
            } else if code.contains("body {") || code.contains("@media") {
                "css"
            } else {
                "txt"
            };
            format!("ai_generated.{}", ext)
        };

        let full_path = root.join(&filename);

        // 确保父目录存在
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }

        // 写入文件
        std::fs::write(&full_path, code).map_err(|e| format!("写入文件失败: {}", e))?;

        // 打开新创建的文件
        self.load_file(full_path.clone());

        self.status_message = format!("已保存文件: {}", filename);
        Ok(full_path)
    }
    /// AI Agent：处理最后一条助手消息中的动作标记（生成完成时调用一次）。
    ///
    /// - `<<<<<<< FILE 路径 >>>>>>>` 块：创建/修改/删除文件（自动建目录）。
    /// - `<<<<<<< RUN >>>>>>>` 块：在集成终端执行命令。
    ///
    /// 执行结果以助手消息形式反馈到 AI 面板，并刷新文件树。
    pub fn process_ai_agent_actions(&mut self) {
        let active = self.ai_panel.active;
        self.process_ai_agent_actions_for(active);
    }

    /// 处理指定会话（conv_idx）刚完成生成时的 Agent 动作：创建/修改文件、执行终端命令。
    /// 支持后台并发会话——反馈写回对应会话，而非总是活动会话。
    pub fn process_ai_agent_actions_for(&mut self, conv_idx: usize) {
        // Edit 模式走差异预览确认流程，不在此处直接落盘。
        if matches!(
            self.ai_panel.mode_of(conv_idx),
            crate::ai_prompt::AiMode::Edit
        ) {
            return;
        }
        let Some(text) = self.ai_panel.last_assistant_text_of(conv_idx) else {
            return;
        };

        // 文件/终端操作必须在已打开的工作区内进行；未打开文件夹时提示用户。
        let has_actions = text.contains("<<<<<<< FILE") || text.contains("<<<<<<< RUN");
        if has_actions && self.current_folder.is_none() {
            self.ai_panel
                .add_assistant_message_to(conv_idx, "提示：尚未打开工作区文件夹，无法直接创建/修改文件。请先通过“文件 → 打开文件夹”打开一个项目再试。".to_string());
            self.dirty_tracker.mark_full_window();
            return;
        }

        // 1. 文件操作（创建/修改/删除）
        let edits = crate::ai_agent::parse_edits(&text, None);
        let mut file_summary: Vec<String> = Vec::new();
        if !edits.is_empty() {
            match self.apply_ai_workspace_edits(&edits) {
                Ok(paths) => {
                    for p in &paths {
                        let name = self
                            .current_folder
                            .as_ref()
                            .and_then(|root| p.strip_prefix(root).ok())
                            .unwrap_or(p.as_path());
                        file_summary.push(format!("✓ 已写入 `{}`", name.display()));
                    }
                }
                Err(e) => {
                    file_summary.push(format!("✕ 文件操作失败: {}", e));
                }
            }
            // 刷新文件树以显示新文件（轻量刷新，保留展开状态，不重启 LSP）
            if self.current_folder.is_some() {
                self.refresh_file_tree_light();
            }
        }

        // 2. 终端命令
        let commands = crate::ai_agent::parse_run_commands(&text);
        let mut cmd_summary: Vec<String> = Vec::new();
        if !commands.is_empty() {
            // 打开底部面板并切换到终端
            self.layout.bottom_panel_visible = true;
            self.bottom_panel_tab = crate::editor::BottomPanelTab::Terminal;
            // 同步终端工作目录到当前工作区
            if let Some(folder) = self.current_folder.clone() {
                self.terminal_panel.cwd = folder.to_string_lossy().to_string();
            }
            // 启动终端（若未运行）并排队命令
            if !self.terminal_panel.running {
                let _ = self.terminal_panel.start();
            }
            for cmd in &commands {
                self.terminal_panel.queue_command(cmd.clone());
                cmd_summary.push(format!("▶ 已执行 `{}`", cmd));
            }
            // 命令可能创建/删除文件：开启一段监视窗口，检测到工作区根目录变化即自动
            // 轻量刷新资源管理器，无需用户手动刷新。
            if self.current_folder.is_some() {
                self.fs_last_root_sig = self.workspace_root_signature();
                self.fs_watch_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(20));
            }
            // 启动终端刷新定时器，保证轮询启动结果并刷新命令队列
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    self.hwnd,
                    crate::window::TERM_TIMER_ID,
                    crate::window::TERM_REFRESH_MS,
                    None,
                );
            }
        }

        // 3. 反馈汇总到对应会话
        if !file_summary.is_empty() || !cmd_summary.is_empty() {
            let mut lines = Vec::new();
            lines.extend(file_summary);
            lines.extend(cmd_summary);
            self.ai_panel
                .add_assistant_message_to(conv_idx, lines.join("\n"));
            self.dirty_tracker.mark_full_window();
        }
    }
    /// 刷新 AI 历史索引。
    /// 从磁盘 conversations/index.json 加载元数据到内存 history。
    pub fn refresh_ai_history(&mut self) {
        if let Some(store) = self.ai_panel.history_store.as_ref() {
            let meta = store.load_history_meta();
            if !meta.is_empty() {
                self.ai_panel.history = meta;
            }
        }
    }

    /// 打开历史记录中的某条会话。
    /// 基础版：从内存 history 恢复会话元数据到新标签页。
    /// Phase 2 将支持从磁盘懒加载 conv-{id}.json 完整消息。
    pub fn open_ai_history_item(&mut self, idx: usize) {
        self.ai_panel.restore_from_history(idx);
    }

    /// 把当前文件的 LSP 诊断发送给 AI 修复
    pub fn ai_fix_diagnostics(&mut self) {
        let settings = self.app_settings.active_ai_settings();
        let context = self.gather_context(&[
            crate::ai_context::AiContextAttachment::CurrentFile,
            crate::ai_context::AiContextAttachment::Diagnostics,
        ]);
        let _ = self.ai_panel.send_message_with_prepared_context(
            &settings,
            context,
            crate::ai_prompt::AiMode::Edit,
        );
    }
    /// 自动应用 AI 面板中待确认的编辑到工作区
    pub fn ai_apply_pending_changes(&mut self) {
        if self.ai_panel.is_generating || self.ai_panel.diff_view.files.is_empty() {
            return;
        }
        let edits = {
            let diff_view = &mut self.ai_panel.diff_view;
            diff_view.accept_all();
            diff_view.to_edits()
        };
        if !edits.is_empty() {
            match self.apply_ai_workspace_edits(&edits) {
                Ok(paths) => {
                    self.status_message = format!("已应用 AI 编辑: {} 个文件", paths.len())
                }
                Err(e) => self.status_message = format!("AI 编辑应用失败: {}", e),
            }
        }
        self.ai_panel.clear_pending_changes();
    }
    /// 接受并立即应用变更列表中的单个文件（变更列表预览“接受”按钮）
    pub fn ai_accept_change_file(&mut self, idx: usize) {
        let edit = match self.ai_panel.diff_view.files.get(idx) {
            Some(f) => crate::ai_agent::AiEdit {
                path: f.path.clone(),
                search: f.original.clone(),
                replace: f.proposed.clone(),
            },
            None => return,
        };
        match self.apply_ai_workspace_edits(&[edit]) {
            Ok(_) => {
                if idx < self.ai_panel.diff_view.files.len() {
                    self.ai_panel.diff_view.files.remove(idx);
                }
                self.status_message = "已应用该文件变更".to_string();
            }
            Err(e) => self.status_message = format!("AI 编辑应用失败: {}", e),
        }
        self.ai_panel.diff_view.selected_index = 0;
        if self.ai_panel.diff_view.files.is_empty() {
            self.ai_panel.clear_pending_changes();
        }
    }
    /// 拒绝变更列表中的单个文件（仅从列表移除，不修改磁盘）
    pub fn ai_reject_change_file(&mut self, idx: usize) {
        if idx < self.ai_panel.diff_view.files.len() {
            self.ai_panel.diff_view.files.remove(idx);
        }
        self.ai_panel.diff_view.selected_index = 0;
        if self.ai_panel.diff_view.files.is_empty() {
            self.ai_panel.clear_pending_changes();
        }
        self.status_message = "已拒绝该文件变更".to_string();
    }
    /// 将设置面板中的 AI 配置应用到 app_settings 并持久化到磁盘
    ///
    /// API 密钥通过 DPAPI 加密单独存储（见 AppSettings::save），不会明文写入 settings.json。
    /// 同时刷新 AI 面板使用的运行时设置。
    pub fn save_ai_settings(&mut self) {
        // 写回激活模型 + 同步模型列表到持久化设置
        self.settings_panel.store_fields_to_active_model();
        self.settings_panel
            .sync_to_app_settings(&mut self.app_settings);
        // 兼容：同时更新旧的单一 ai 字段（作为无模型时的回退）
        self.app_settings.ai = self.settings_panel.to_ai_settings();
        match self.app_settings.save() {
            Ok(_) => {
                self.settings_panel.mark_saved();
                self.settings_panel.test_status = "✓ 设置已保存".to_string();
                self.status_message = "AI 设置已保存".to_string();
            }
            Err(e) => {
                self.settings_panel.test_status = format!("✗ 保存失败：{}", e);
            }
        }
    }
    /// 持久化模型列表变更（删除/启用切换/设为激活/新建后调用）
    pub fn persist_models(&mut self) {
        self.settings_panel
            .sync_to_app_settings(&mut self.app_settings);
        if let Err(e) = self.app_settings.save() {
            self.settings_panel.test_status = format!("✗ 保存失败：{}", e);
        }
    }
    /// 保存 AI 设置前，先启动测试连接验证密钥有效性。
    /// 测试成功后会自动调用 save_ai_settings 完成保存。
    pub fn save_ai_settings_with_test(&mut self) {
        let ai = self.settings_panel.to_ai_settings();
        if ai.api_key.trim().is_empty() {
            self.settings_panel.test_status = "✗ 请先填写 API 密钥".to_string();
            return;
        }
        self.settings_panel.pending_save = true;
        self.settings_panel.start_test_connection(ai);
    }
    /// 使用设置面板当前配置启动 AI 测试连接（后台线程，不阻塞 UI）
    pub fn start_ai_test_connection(&mut self) {
        let ai = self.settings_panel.to_ai_settings();
        self.settings_panel.start_test_connection(ai);
    }
    /// 根据附件列表收集 AI 上下文
    pub fn gather_context(&self, attachments: &[AiContextAttachment]) -> String {
        let mut parts = Vec::new();
        let current_path = self
            .content
            .file_path
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "当前文件".to_string());
        let current_lang = language_str(self.content.language);

        for attachment in attachments {
            match attachment {
                AiContextAttachment::CurrentFile => {
                    let text = self
                        .content
                        .buffer
                        .get_text(0, self.content.buffer.len_bytes());
                    parts.push(wrap_code_block(
                        &current_path,
                        current_lang,
                        &truncate_middle(&text, 30_000),
                    ));
                }
                AiContextAttachment::Selection => {
                    if let Some(text) = self.selected_text() {
                        parts.push(wrap_code_block(
                            &format!("{} (选区)", current_path),
                            current_lang,
                            &truncate_middle(&text, 10_000),
                        ));
                    }
                }
                AiContextAttachment::OpenFiles => {
                    let mut summary = String::from("打开的文件列表：\n");
                    // 活动标签页的内容存于 self.content（swap 后），需提前提取避免借用冲突
                    let active_idx = self.active_tab;
                    let active_path = self
                        .content
                        .file_path
                        .as_deref()
                        .map(|p| p.to_string_lossy().to_string());
                    let active_lang = language_str(self.content.language);
                    let active_text = self
                        .content
                        .buffer
                        .get_text(0, self.content.buffer.len_bytes());
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let (path, lang, text) = if i == active_idx {
                            (
                                active_path
                                    .clone()
                                    .unwrap_or_else(|| format!("未命名-{}", i + 1)),
                                active_lang,
                                active_text.clone(),
                            )
                        } else if let Some(content) = tab.as_file() {
                            let path = content
                                .file_path
                                .as_deref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| format!("未命名-{}", i + 1));
                            let lang = language_str(content.language);
                            let text = content.buffer.get_text(0, content.buffer.len_bytes());
                            (path, lang, text)
                        } else {
                            continue;
                        };
                        summary.push_str(&wrap_code_block(
                            &path,
                            lang,
                            &truncate_middle(&text, 5_000),
                        ));
                    }
                    parts.push(summary);
                }
                AiContextAttachment::Diagnostics => {
                    let current_key = self
                        .content
                        .file_path
                        .as_deref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let mut all: Vec<&DiagnosticItem> =
                        self.diagnostics.values().flatten().collect();
                    // 优先显示当前文件，再按 severity 排序（1=Error, 2=Warning）
                    all.sort_by_key(|d| {
                        let is_current = self
                            .content
                            .file_path
                            .as_deref()
                            .map(|p| p.to_string_lossy().to_string() == current_key)
                            .unwrap_or(false);
                        (if is_current { 0 } else { 1 }, d.severity)
                    });
                    if all.is_empty() {
                        parts.push("当前文件暂无 LSP 诊断信息。\n".to_string());
                    } else {
                        let mut text = String::from("当前 LSP 诊断：\n");
                        for d in all.iter().take(20) {
                            let severity = match d.severity {
                                1 => "Error",
                                2 => "Warning",
                                3 => "Information",
                                4 => "Hint",
                                _ => "Diagnostic",
                            };
                            text.push_str(&format!(
                                "[{}] {}:{} {}\n",
                                severity, d.line, d.col, d.message
                            ));
                        }
                        parts.push(text);
                    }
                }
                AiContextAttachment::FileTree => {
                    if let Some(tree) = &self.file_tree {
                        parts.push(format!("工作区文件树：\n{}\n", self.format_file_tree(tree)));
                    } else {
                        parts.push("未加载工作区文件树。\n".to_string());
                    }
                }
                AiContextAttachment::CustomText(text) => {
                    parts.push(format!("用户附加文本：\n{}\n", text));
                }
            }
        }

        parts.join("\n")
    }
    /// 应用 AI 生成的代码到当前编辑器
    pub fn apply_ai_code(&mut self, code: &str) -> bool {
        if code.is_empty() {
            return false;
        }
        // 如果有选区，替换选区内容；否则在当前光标位置插入
        // C-02/H-21: 使用 zip 一次性解构，避免独立 unwrap 在中间状态变更后 panic
        if let Some(((start_line, start_col), (end_line, end_col))) =
            self.content.selection_start.zip(self.content.selection_end)
        {
            let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
                (start_line, start_col)
            } else {
                (end_line, end_col)
            };
            let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
                (end_line, end_col)
            } else {
                (start_line, start_col)
            };
            let start_byte = self.line_byte_start(first_line) + first_col;
            let end_byte = self.line_byte_start(last_line) + last_col;

            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(start_byte, end_byte);
            self.content.buffer.insert(start_byte, code);

            // 计算新光标位置
            let code_lines: Vec<&str> = code.lines().collect();
            let new_line = first_line + code_lines.len().saturating_sub(1);
            let new_col = if code_lines.len() <= 1 {
                first_col + code.len()
            } else {
                code_lines.last().unwrap_or(&"").len()
            };
            self.content.cursor_line = new_line;
            self.content.cursor_col = new_col;
            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                start_byte,
                code.len(),
            );

            self.clear_selection();
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已应用 AI 代码".to_string();
            return true;
        }
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        self.content.buffer.insert(pos, code);

        // 更新光标位置
        let _code_lines: Vec<&str> = code.lines().collect();
        let line_breaks = code.matches('\n').count();
        if line_breaks == 0 {
            self.content.cursor_col += code.len();
        } else {
            self.content.cursor_line += line_breaks;
            self.content.cursor_col = code
                .rsplit_once('\n')
                .map(|(_, last)| last.len())
                .unwrap_or(0);
        }
        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            code.len(),
        );

        self.content.is_dirty = true;
        self.content.buffer_version += 1;
        self.status_message = "已插入 AI 代码".to_string();
        true
    }
    /// 应用 AI 生成的工作区编辑（支持修改已打开/未打开的文件以及创建新文件）
    pub fn apply_ai_workspace_edits(
        &mut self,
        edits: &[AiEdit],
    ) -> std::result::Result<Vec<PathBuf>, String> {
        let mut applied = Vec::new();
        let original_tab = self.active_tab;

        for edit in edits {
            let full_path = self.resolve_edit_path(&edit.path);

            // 删除文件操作
            if edit.is_delete() {
                // 关闭对应 tab（如果有）；用户取消则跳过此文件
                if let Some(idx) = self
                    .tabs
                    .iter()
                    .position(|t| t.file_path() == Some(&full_path))
                {
                    if !self.close_tab(idx) {
                        continue;
                    }
                }
                // 从磁盘删除文件
                if full_path.exists() {
                    std::fs::remove_file(&full_path)
                        .map_err(|e| format!("删除文件 {} 失败: {}", full_path.display(), e))?;
                }
                self.status_message = format!("已删除文件: {}", full_path.display());
                applied.push(full_path);
                continue;
            }

            // 找到或创建对应标签页
            let tab_idx = self
                .tabs
                .iter()
                .position(|t| t.file_path() == Some(&full_path));
            if let Some(idx) = tab_idx {
                self.switch_tab(idx);
            } else if full_path.exists() {
                self.load_file(full_path.clone());
            } else {
                self.create_new_file_tab(&full_path);
            }

            // 应用单个编辑
            let old_text = self
                .content
                .buffer
                .get_text(0, self.content.buffer.len_bytes());
            let new_text = if edit.search.trim().is_empty() {
                edit.replace.clone()
            } else {
                match old_text.find(&edit.search) {
                    Some(pos) => {
                        let mut replaced = old_text.clone();
                        replaced.replace_range(pos..pos + edit.search.len(), &edit.replace);
                        replaced
                    }
                    None => {
                        return Err(format!(
                            "无法在 {} 中找到要替换的代码片段",
                            full_path.display()
                        ));
                    }
                }
            };

            // 记录 undo history，使 AI 工作区编辑可通过 Ctrl+Z 逐文件撤销
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            let len = self.content.buffer.len_bytes();
            self.content.buffer.delete(0, len);
            self.content.buffer.insert(0, &new_text);
            // 全量替换后将光标复位到文件开头，避免越界
            self.content.cursor_line = 0;
            self.content.cursor_col = 0;
            let cursor_after = CursorPosition::new(0, 0);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                0,
                new_text.len(),
            );
            self.content.buffer_version += 1;

            // 关键：将内容实际写入磁盘（当前工作区），而非仅停留在内存缓冲。
            // 先确保父目录存在（支持多级子目录自动创建），再原子写入。
            if let Some(parent) = full_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Err(format!("创建目录 {} 失败: {}", parent.display(), e));
                }
            }
            if let Err(e) = Self::atomic_write(&full_path, new_text.as_bytes()) {
                return Err(format!("写入文件 {} 失败: {}", full_path.display(), e));
            }
            // 已落盘，清除脏标记
            self.content.is_dirty = false;
            self.status_message = format!("已写入文件: {}", full_path.display());
            applied.push(full_path);
        }

        // 尽量回到原来的标签页
        if original_tab < self.tabs.len() {
            self.switch_tab(original_tab);
        }

        Ok(applied)
    }
    pub(crate) fn resolve_edit_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        self.current_folder
            .as_ref()
            .map(|root| root.join(path))
            .unwrap_or_else(|| path.to_path_buf())
    }
}
