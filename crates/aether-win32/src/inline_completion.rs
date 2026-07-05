/// Inline Completion（幽灵文本）状态管理
///
/// P3.1: 为 AI 写代码提供最小数据结构。当前不绑定具体 AI provider，
/// 只保存建议文本、触发位置、接受状态，并提供生命周期控制。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineCompletion {
    /// 建议插入的完整文本
    pub text: String,
    /// 触发建议时的光标行
    pub trigger_line: usize,
    /// 触发建议时的光标列（字节偏移）
    pub trigger_col: usize,
    /// 建议版本号，用于区分新旧建议
    pub version: u64,
}

impl InlineCompletion {
    pub fn new(text: String, trigger_line: usize, trigger_col: usize, version: u64) -> Self {
        Self {
            text,
            trigger_line,
            trigger_col,
            version,
        }
    }

    /// 建议是否为空
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// Inline Completion 服务占位
///
/// 实际实现中应连接 aether-ai crate，异步请求模型并回调。
/// 当前阶段仅提供同步 API 形状，方便 UI 层先跑起来。
pub struct InlineCompletionService {
    counter: u64,
}

impl InlineCompletionService {
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    /// 根据当前上下文请求建议（占位实现）
    ///
    /// 返回 Some(...) 表示本地模拟建议；生产环境应改为异步 Future。
    pub fn request(&mut self, _prefix: &str, _suffix: &str) -> Option<InlineCompletion> {
        // P3.1: 占位——返回一个可见的模拟建议，便于 UI 调试
        self.counter += 1;
        Some(InlineCompletion::new(
            "// AI suggestion".to_string(),
            0,
            0,
            self.counter,
        ))
    }

    /// 取消当前请求（占位）
    pub fn cancel(&mut self) {
        // 占位：异步实现时取消 in-flight 请求
    }
}

impl Default for InlineCompletionService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_completion_new_and_fields() {
        let comp = InlineCompletion::new("hello".to_string(), 3, 5, 42);
        assert_eq!(comp.text, "hello");
        assert_eq!(comp.trigger_line, 3);
        assert_eq!(comp.trigger_col, 5);
        assert_eq!(comp.version, 42);
    }

    #[test]
    fn test_inline_completion_is_empty() {
        let empty = InlineCompletion::new(String::new(), 0, 0, 1);
        assert!(empty.is_empty());

        let non_empty = InlineCompletion::new("x".to_string(), 0, 0, 1);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_service_request_returns_some_with_incrementing_version() {
        let mut svc = InlineCompletionService::new();
        let r1 = svc.request("prefix", "suffix").expect("应返回建议");
        let r2 = svc.request("prefix", "suffix").expect("应返回建议");
        assert!(r2.version > r1.version, "版本号应递增");
        assert!(!r1.text.is_empty(), "占位建议不应为空文本");
    }

    #[test]
    fn test_service_default_equals_new() {
        let mut a = InlineCompletionService::new();
        let mut b = InlineCompletionService::default();
        // 两者起始 counter 相同，第一次 request 返回的 version 应一致
        let ra = a.request("", "");
        let rb = b.request("", "");
        assert_eq!(ra.map(|c| c.version), rb.map(|c| c.version));
    }

    #[test]
    fn test_service_cancel_is_noop() {
        let mut svc = InlineCompletionService::new();
        // cancel 在占位实现中不应 panic
        svc.cancel();
        // cancel 后仍可正常 request
        assert!(svc.request("a", "b").is_some());
    }
}
