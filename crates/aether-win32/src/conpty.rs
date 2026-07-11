//! ConPTY（Windows 伪控制台）封装与普通管道回退。
//!
//! 优先使用 `CreatePseudoConsole` API 创建真正的同步终端，支持交互式命令、
//! Tab 补全、方向键导航、ANSI 颜色等。当 ConPTY 不可用时（例如 GUI 子系统
//! 进程无法分配控制台），回退到传统匿名管道子进程，仍可输入输出。
//!
//! 注意：GUI 子系统（`#![windows_subsystem = "windows"]`）进程在调用
//! `CreatePseudoConsole` 前需要关联控制台，否则返回 `E_HANDLE`。
//! 本模块会尝试在 `spawn` 时分配隐藏控制台；若失败则回退到管道。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{
    AllocConsole, ClosePseudoConsole, CreatePseudoConsole, FreeConsole, GetConsoleProcessList,
    GetConsoleWindow, ResizePseudoConsole, COORD, HPCON,
};
use windows::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HeapFree, HEAP_FLAGS};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
use windows::core::PCWSTR;

/// ConPTY 进程属性值。Windows SDK 中 `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` = 0x20016。
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x20016;

/// 终端后端类型：ConPTY（功能完整）或 普通管道（回退）。
enum Backend {
    ConPty {
        hpc: HPCON,
        /// 本会话是否由 `spawn` 分配了控制台，Drop 时需要释放。
        console_allocated: bool,
    },
    Pipe,
}

/// ConPTY / 管道 会话，封装子进程的完整生命周期。
///
/// 创建后，调用方通过 `write_input` 发送输入，
/// 通过返回的读取管道 `HANDLE` 在后台线程中读取输出。
/// `Drop` 时自动清理所有资源。
pub struct ConPtySession {
    backend: Backend,
    process_info: PROCESS_INFORMATION,
    pipe_write: HANDLE,
    /// 属性列表缓冲区（堆分配，Drop 时释放）。仅 ConPTY 模式使用。
    attr_buffer: *mut std::ffi::c_void,
    attr_initialized: bool,
}

unsafe impl Send for ConPtySession {}

impl ConPtySession {
    /// 创建 ConPTY 会话并启动子进程。
    ///
    /// 返回 `(session, read_handle)`，`read_handle` 是输出读取管道句柄，
    /// 调用方应在后台线程中读取并在结束时 `CloseHandle`。
    ///
    /// 本函数会尝试在 GUI 子系统下分配隐藏控制台以使用 ConPTY；
    /// 若分配失败则回退到普通管道子进程。
    pub fn spawn(
        commandline: &str,
        cwd: Option<&str>,
        cols: i16,
        rows: i16,
    ) -> Result<(ConPtySession, HANDLE), String> {
        unsafe {
            // 尝试为 ConPTY 分配隐藏控制台。
            //
            // GUI 子系统进程没有控制台。某些启动环境（如从 Trae/VS Code 终端
            // 启动）会继承父进程的无效控制台句柄，导致 AllocConsole 返回
            // E_HANDLE。解决方法：先 FreeConsole 清理残留句柄，再 AllocConsole。
            //
            // 注意：仅当 GetConsoleWindow 返回 null 且 GetConsoleProcessList
            // 返回 0 时（真正无 console），才调用 FreeConsole。
            // 如果已经 attached to parent's console（GetConsoleProcessList > 0），
            // 直接使用现有 console，不要 FreeConsole（会断开与父进程的关系）。
            let mut console_allocated = false;
            let existing_console_window = GetConsoleWindow();
            let mut pids = [0u32; 1];
            let inherited_console = GetConsoleProcessList(pids.as_mut_slice()) > 0;
            if !existing_console_window.0.is_null() || inherited_console {
                // 已有 console（可能是从命令行/父进程继承），直接使用
                tracing::info!(
                    has_window = !existing_console_window.0.is_null(),
                    inherited = inherited_console,
                    "ConPTY: 检测到已有控制台，直接使用"
                );
            } else {
                // 先 FreeConsole 清理可能继承的无效控制台句柄
                let _ = FreeConsole();
                match AllocConsole() {
                    Ok(_) => {
                        let hwnd = GetConsoleWindow();
                        if !hwnd.0.is_null() {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }
                        console_allocated = true;
                        tracing::info!("ConPTY: AllocConsole 成功（FreeConsole 后重新分配）");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "ConPTY: AllocConsole 失败（FreeConsole 后仍失败），将回退到普通管道");
                    }
                }
            }

            // 尝试 ConPTY 创建
            if console_allocated || !existing_console_window.0.is_null() || inherited_console {
                match Self::spawn_conpty(commandline, cwd, cols, rows, console_allocated) {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        tracing::warn!(error = %e, "ConPTY 创建失败，回退到普通管道");
                        if console_allocated {
                            let _ = FreeConsole();
                        }
                    }
                }
            }

            // 回退：使用普通管道启动子进程
            tracing::info!("ConPTY 不可用，使用普通管道模式");
            Self::spawn_pipe(commandline, cwd)
        }
    }

    /// ConPTY 路径：创建伪控制台并启动子进程。
    fn spawn_conpty(
        commandline: &str,
        cwd: Option<&str>,
        cols: i16,
        rows: i16,
        console_allocated: bool,
    ) -> Result<(ConPtySession, HANDLE), String> {
        unsafe {
            // 1. 创建两对匿名管道。
            //    bInheritHandle = TRUE：管道句柄需要可继承，以便 CreateProcessW
            //    将子进程的标准输入/输出关联到这些管道（Microsoft 官方示例）。
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: windows::Win32::Foundation::BOOL(1),
            };
            let mut pipe_in_read = INVALID_HANDLE_VALUE;
            let mut pipe_in_write = INVALID_HANDLE_VALUE;
            CreatePipe(&mut pipe_in_read, &mut pipe_in_write, Some(&sa), 0)
                .map_err(|e| format!("CreatePipe(input) 失败: {}", e))?;

            let mut pipe_out_read = INVALID_HANDLE_VALUE;
            let mut pipe_out_write = INVALID_HANDLE_VALUE;
            CreatePipe(&mut pipe_out_read, &mut pipe_out_write, Some(&sa), 0).map_err(|e| {
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                format!("CreatePipe(output) 失败: {}", e)
            })?;

            // 2. 创建伪控制台（返回 HPCON 而非通过指针参数）
            let size = COORD { X: cols, Y: rows };
            let hpc = CreatePseudoConsole(size, pipe_in_read, pipe_out_write, 0).map_err(|e| {
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                format!("CreatePseudoConsole 失败: {}", e)
            })?;

            // 3. 准备进程属性列表
            let mut attr_size: usize = 0;
            // 第一次调用获取所需大小（传 null 列表，返回错误但设置 size）
            let _ = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                1,
                0,
                &mut attr_size,
            );

            let heap = GetProcessHeap().map_err(|e| {
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                format!("GetProcessHeap 失败: {}", e)
            })?;
            let attr_buffer = HeapAlloc(heap, HEAP_FLAGS(0), attr_size);
            if attr_buffer.is_null() {
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                return Err("HeapAlloc 属性列表失败".to_string());
            }

            let attr_list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buffer);
            if let Err(e) = InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) {
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attr_buffer));
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                return Err(format!("InitializeProcThreadAttributeList 失败: {}", e));
            }

            // 设置 PSEUDOCONSOLE 属性，将子进程关联到 ConPTY
            // 注意：lpValue 传递 HPCON 句柄值本身（而非指向它的指针），
            // 这与 Microsoft C 示例 UpdateProcThreadAttribute(..., hPC, sizeof(HPCON), ...) 一致
            if let Err(e) = UpdateProcThreadAttribute(
                attr_list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                Some(hpc.0 as *const std::ffi::c_void),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            ) {
                DeleteProcThreadAttributeList(attr_list);
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attr_buffer));
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                return Err(format!("UpdateProcThreadAttribute 失败: {}", e));
            }

            // 4. 准备 STARTUPINFOEXW
            //
            //    **重要**：使用 ConPTY 时不能设置 STARTF_USESTDHANDLES，也不能
            //    设置 hStdInput/hStdOutput/hStdError。ConPTY 会自动为子进程创建
            //    控制台并管理其 stdio。设置 STARTF_USESTDHANDLES 会导致子进程
            //    直接使用管道句柄而非 ConPTY 控制台，进入非交互模式（不回显）。
            //
            //    同时 bInheritHandles 必须为 FALSE（Microsoft 官方要求），
            //    因为 PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE 与句柄继承互斥。
            let startup_info = STARTUPINFOEXW {
                StartupInfo: STARTUPINFOW {
                    cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
                    ..Default::default()
                },
                lpAttributeList: attr_list,
            };

            let (mut cmdline_wide, cwd_wide) = Self::build_command_line(commandline, cwd);

            // 5. 创建子进程
            //    bInheritHandles = FALSE：ConPTY 属性与句柄继承互斥（Microsoft 官方要求）。
            let mut process_info = PROCESS_INFORMATION::default();
            let result = CreateProcessW(
                None,
                windows::core::PWSTR(cmdline_wide.as_mut_ptr()),
                None,
                None,
                false,
                EXTENDED_STARTUPINFO_PRESENT,
                None,
                cwd_wide
                    .as_ref()
                    .map(|v| PCWSTR(v.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                &startup_info.StartupInfo as *const STARTUPINFOW,
                &mut process_info,
            );

            if let Err(e) = result {
                DeleteProcThreadAttributeList(attr_list);
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attr_buffer));
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                return Err(format!("CreateProcessW 失败: {}", e));
            }

            // 子进程已创建，父进程关闭它复制出的管道端句柄。
            let _ = CloseHandle(pipe_in_read);
            let _ = CloseHandle(pipe_out_write);

            // 关闭子进程线程句柄（不需要）
            let _ = CloseHandle(process_info.hThread);
            process_info.hThread = HANDLE::default();

            // 等待 200ms 检查子进程是否立即崩溃
            let wait_result = WaitForSingleObject(process_info.hProcess, 200);
            if wait_result.0 == 0 {
                let mut exit_code: u32 = 0;
                let _ = GetExitCodeProcess(process_info.hProcess, &mut exit_code);
                let _ = CloseHandle(process_info.hProcess);
                ClosePseudoConsole(hpc);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                DeleteProcThreadAttributeList(attr_list);
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attr_buffer));
                return Err(format!(
                    "子进程立即退出（退出码 0x{:08X}），可能 ConPTY 配置不兼容",
                    exit_code
                ));
            }

            tracing::info!("ConPTY 子进程启动成功");

            Ok((
                ConPtySession {
                    backend: Backend::ConPty {
                        hpc,
                        console_allocated,
                    },
                    process_info,
                    pipe_write: pipe_in_write,
                    attr_buffer,
                    attr_initialized: true,
                },
                pipe_out_read,
            ))
        }
    }

    /// 普通管道路径：当 ConPTY 不可用时，使用传统管道启动子进程。
    fn spawn_pipe(commandline: &str, cwd: Option<&str>) -> Result<(ConPtySession, HANDLE), String> {
        unsafe {
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: windows::Win32::Foundation::BOOL(1),
            };

            let mut pipe_in_read = INVALID_HANDLE_VALUE;
            let mut pipe_in_write = INVALID_HANDLE_VALUE;
            CreatePipe(&mut pipe_in_read, &mut pipe_in_write, Some(&sa), 0)
                .map_err(|e| format!("CreatePipe(input) 失败: {}", e))?;

            let mut pipe_out_read = INVALID_HANDLE_VALUE;
            let mut pipe_out_write = INVALID_HANDLE_VALUE;
            CreatePipe(&mut pipe_out_read, &mut pipe_out_write, Some(&sa), 0).map_err(|e| {
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                format!("CreatePipe(output) 失败: {}", e)
            })?;

            let startup_info = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: pipe_in_read,
                hStdOutput: pipe_out_write,
                hStdError: pipe_out_write,
                ..Default::default()
            };

            let (mut cmdline_wide, cwd_wide) = Self::build_command_line(commandline, cwd);

            let mut process_info = PROCESS_INFORMATION::default();
            let result = CreateProcessW(
                None,
                windows::core::PWSTR(cmdline_wide.as_mut_ptr()),
                None,
                None,
                true,
                windows::Win32::System::Threading::CREATE_NO_WINDOW,
                None,
                cwd_wide
                    .as_ref()
                    .map(|v| PCWSTR(v.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                &startup_info as *const STARTUPINFOW,
                &mut process_info,
            );

            if let Err(e) = result {
                let _ = CloseHandle(pipe_in_read);
                let _ = CloseHandle(pipe_in_write);
                let _ = CloseHandle(pipe_out_read);
                let _ = CloseHandle(pipe_out_write);
                return Err(format!("CreateProcessW 失败: {}", e));
            }

            // 关闭父进程不需要的端句柄
            let _ = CloseHandle(pipe_in_read);
            let _ = CloseHandle(pipe_out_write);
            let _ = CloseHandle(process_info.hThread);
            process_info.hThread = HANDLE::default();

            tracing::info!("普通管道子进程启动成功");

            Ok((
                ConPtySession {
                    backend: Backend::Pipe,
                    process_info,
                    pipe_write: pipe_in_write,
                    attr_buffer: std::ptr::null_mut(),
                    attr_initialized: false,
                },
                pipe_out_read,
            ))
        }
    }

    /// 构建宽字符命令行和当前目录。
    fn build_command_line(commandline: &str, cwd: Option<&str>) -> (Vec<u16>, Option<Vec<u16>>) {
        let quoted = if commandline.contains(' ') && !commandline.starts_with('"') {
            format!("\"{}\"", commandline)
        } else {
            commandline.to_string()
        };
        let mut cmdline_wide: Vec<u16> = OsStr::new(&quoted).encode_wide().collect();
        cmdline_wide.push(0);

        let cwd_wide: Option<Vec<u16>> = cwd.map(|c| {
            let mut v: Vec<u16> = OsStr::new(c).encode_wide().collect();
            v.push(0);
            v
        });

        (cmdline_wide, cwd_wide)
    }

    /// 向子进程写入输入（发送给子进程 stdin）。
    pub fn write_input(&self, data: &[u8]) -> Result<(), String> {
        unsafe {
            let mut written: u32 = 0;
            WriteFile(
                self.pipe_write,
                Some(data),
                Some(&mut written as *mut u32),
                None,
            )
            .map_err(|e| format!("WriteFile 失败: {}", e))?;
        }
        Ok(())
    }

    /// 调整终端尺寸。ConPTY 模式下真正调整尺寸；管道模式下仅记录（无效果）。
    pub fn resize(&mut self, cols: i16, rows: i16) -> Result<(), String> {
        unsafe {
            match &self.backend {
                Backend::ConPty { hpc, .. } => {
                    ResizePseudoConsole(*hpc, COORD { X: cols, Y: rows })
                        .map_err(|e| format!("ResizePseudoConsole 失败: {}", e))
                }
                Backend::Pipe => Ok(()),
            }
        }
    }

    /// 是否为普通管道模式（非 ConPTY）。
    /// 管道模式下没有伪控制台，子进程不会有交互式回显和行编辑。
    pub fn is_pipe(&self) -> bool {
        matches!(self.backend, Backend::Pipe)
    }

    /// 检查子进程是否仍在运行。
    pub fn is_alive(&self) -> bool {
        unsafe {
            let mut exit_code: u32 = 0;
            if GetExitCodeProcess(self.process_info.hProcess, &mut exit_code).is_err() {
                return false;
            }
            exit_code == 259 // STILL_ACTIVE
        }
    }

    /// 获取子进程退出码。若进程仍在运行返回 None。
    pub fn exit_code(&self) -> Option<u32> {
        unsafe {
            let mut exit_code: u32 = 0;
            GetExitCodeProcess(self.process_info.hProcess, &mut exit_code).ok()?;
            if exit_code == 259 {
                None
            } else {
                Some(exit_code)
            }
        }
    }
}

impl Drop for ConPtySession {
    fn drop(&mut self) {
        unsafe {
            if !self.pipe_write.is_invalid() {
                let _ = CloseHandle(self.pipe_write);
            }

            match &self.backend {
                Backend::ConPty { hpc, console_allocated } => {
                    // 先关闭 ConPTY，通知子进程控制台已断开
                    ClosePseudoConsole(*hpc);
                    // 等待子进程自行退出
                    let wait = WaitForSingleObject(self.process_info.hProcess, 500);
                    if wait.0 != 0 {
                        let _ = TerminateProcess(self.process_info.hProcess, 1);
                        let _ = WaitForSingleObject(self.process_info.hProcess, 1000);
                    }
                    if !self.process_info.hProcess.is_invalid() {
                        let _ = CloseHandle(self.process_info.hProcess);
                    }
                    // 若本会话分配了控制台，在子进程结束后释放
                    if *console_allocated {
                        let _ = FreeConsole();
                    }
                }
                Backend::Pipe => {
                    // 普通管道：直接等待并终止子进程
                    let wait = WaitForSingleObject(self.process_info.hProcess, 500);
                    if wait.0 != 0 {
                        let _ = TerminateProcess(self.process_info.hProcess, 1);
                        let _ = WaitForSingleObject(self.process_info.hProcess, 1000);
                    }
                    if !self.process_info.hProcess.is_invalid() {
                        let _ = CloseHandle(self.process_info.hProcess);
                    }
                }
            }

            if self.attr_initialized {
                DeleteProcThreadAttributeList(LPPROC_THREAD_ATTRIBUTE_LIST(self.attr_buffer));
            }
            if !self.attr_buffer.is_null() {
                if let Ok(heap) = GetProcessHeap() {
                    let _ = HeapFree(heap, HEAP_FLAGS(0), Some(self.attr_buffer));
                }
            }
        }
    }
}

/// 从 Windows `HANDLE` 读取数据的适配器，实现 `std::io::Read`。
pub struct PipeReader {
    handle: HANDLE,
}

// SAFETY: PipeReader 独占拥有管道读取句柄，仅在单一后台线程中读取，
// 不跨线程共享，因此可以安全地 Send。
unsafe impl Send for PipeReader {}

impl PipeReader {
    pub fn new(handle: HANDLE) -> Self {
        Self { handle }
    }
}

impl std::io::Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        unsafe {
            let mut bytes_read: u32 = 0;
            ReadFile(
                self.handle,
                Some(buf),
                Some(&mut bytes_read as *mut u32),
                None,
            )
            .map_err(|e| std::io::Error::other(format!("ReadFile 失败: {}", e)))?;
            Ok(bytes_read as usize)
        }
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_invalid() {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}
