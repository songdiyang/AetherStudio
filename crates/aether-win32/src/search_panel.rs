use aether_core::search::{search_workspace, SearchQuery, SearchResult};

/// 底部全局搜索面板
#[derive(Clone, Debug)]
pub struct SearchPanel {
    pub visible: bool,
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub results: Vec<SearchResult>,
    pub selected_index: usize,
    pub is_searching: bool,
    pub status: String,
}

impl SearchPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            regex: false,
            case_sensitive: false,
            results: Vec::new(),
            selected_index: 0,
            is_searching: false,
            status: String::new(),
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn input_char(&mut self, ch: char) {
        self.query.push(ch);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.results.clear();
        self.selected_index = 0;
        self.status.clear();
    }

    /// 在指定工作区目录执行搜索
    pub fn search(&mut self, root_dir: Option<&std::path::Path>) {
        if self.query.is_empty() {
            self.results.clear();
            self.status = "请输入搜索内容".to_string();
            return;
        }
        let Some(root) = root_dir else {
            self.status = "未打开工作区".to_string();
            return;
        };
        self.is_searching = true;
        let query = SearchQuery {
            pattern: self.query.clone(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            ..Default::default()
        };
        self.results = search_workspace(root, &query);
        self.selected_index = 0;
        self.status = format!("找到 {} 个结果", self.results.len());
        self.is_searching = false;
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.results.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.results.is_empty() {
            self.selected_index =
                (self.selected_index + self.results.len() - 1) % self.results.len();
        }
    }

    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
    }

    /// 切换正则选项
    pub fn toggle_regex(&mut self) {
        self.regex = !self.regex;
    }

    /// 切换大小写敏感
    pub fn toggle_case_sensitive(&mut self) {
        self.case_sensitive = !self.case_sensitive;
    }
}
