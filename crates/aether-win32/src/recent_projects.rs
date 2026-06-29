use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 最近项目记录
#[derive(Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
    pub last_opened: SystemTime,
}

impl RecentProject {
    /// 从路径创建最近项目记录，自动提取名称
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        let path_str = path.to_string_lossy().to_string();
        Self {
            name,
            path: path_str,
            last_opened: SystemTime::now(),
        }
    }
}

/// 最近项目列表管理器
/// 持久化存储在 %APPDATA%/Aether/recent_projects.json
pub struct RecentProjectsManager {
    projects: VecDeque<RecentProject>,
    config_dir: PathBuf,
    max_count: usize,
}

impl RecentProjectsManager {
    const MAX_COUNT: usize = 5;
    const FILE_NAME: &'static str = "recent_projects.json";

    pub fn new() -> Self {
        let config_dir = Self::config_dir();
        let mut manager = Self {
            projects: VecDeque::new(),
            config_dir,
            max_count: Self::MAX_COUNT,
        };
        manager.load();
        // 启动时清理已失效（不存在的）项目
        manager.clean_invalid();
        manager
    }

    /// 添加或更新最近项目（移动到最前面）
    pub fn add(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();

        // 如果已存在，先移除旧记录
        self.projects.retain(|p| p.path != path_str);

        // 添加新记录到最前面
        let project = RecentProject::from_path(path);
        self.projects.push_front(project);

        // 限制数量
        while self.projects.len() > self.max_count {
            self.projects.pop_back();
        }

        // 持久化
        let _ = self.save();
    }

    /// 获取最近项目列表（按时间倒序）
    pub fn list(&self) -> Vec<RecentProject> {
        self.projects.iter().cloned().collect()
    }

    /// 判断是否有最近项目
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    /// 获取项目数量
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// 移除不存在的项目
    pub fn clean_invalid(&mut self) {
        self.projects.retain(|p| Path::new(&p.path).exists());
        let _ = self.save();
    }

    fn config_dir() -> PathBuf {
        let app_data = std::env::var("APPDATA")
            .or_else(|_| std::env::var("HOME"))
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(app_data).join("Aether");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn file_path(&self) -> PathBuf {
        self.config_dir.join(Self::FILE_NAME)
    }

    fn load(&mut self) {
        let path = self.file_path();
        let Ok(mut file) = fs::File::open(&path) else {
            return;
        };
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_err() {
            return;
        }

        // 简单解析 JSON 数组格式
        self.projects = Self::parse_json(&contents);
    }

    fn save(&self) -> io::Result<()> {
        let path = self.file_path();
        let mut file = fs::File::create(&path)?;
        let json = Self::to_json(&self.projects);
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// 简单 JSON 序列化
    fn to_json(projects: &VecDeque<RecentProject>) -> String {
        let mut json = String::from("[\n");
        for (i, p) in projects.iter().enumerate() {
            let last_opened_secs = p
                .last_opened
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            json.push_str("  {\n");
            json.push_str(&format!(
                "    \"name\": \"{}\",\n",
                Self::escape_json(&p.name)
            ));
            json.push_str(&format!(
                "    \"path\": \"{}\",\n",
                Self::escape_json(&p.path)
            ));
            json.push_str(&format!("    \"last_opened\": {}\n", last_opened_secs));
            json.push('}');
            if i < projects.len() - 1 {
                json.push(',');
            }
            json.push('\n');
        }
        json.push(']');
        json
    }

    /// 简单 JSON 解析
    fn parse_json(json: &str) -> VecDeque<RecentProject> {
        let mut projects = VecDeque::new();
        let mut current_name = None;
        let mut current_path = None;
        let mut current_last_opened = None;

        for line in json.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("\"name\"") {
                if let Some(val) = Self::extract_json_value(trimmed) {
                    current_name = Some(val);
                }
            } else if trimmed.starts_with("\"path\"") {
                if let Some(val) = Self::extract_json_value(trimmed) {
                    current_path = Some(val);
                }
            } else if trimmed.starts_with("\"last_opened\"") {
                // 解析数值字段
                let colon_idx = match trimmed.find(':') {
                    Some(idx) => idx,
                    None => continue,
                };
                let after_colon = trimmed[colon_idx + 1..].trim();
                if let Ok(secs) = after_colon.trim_end_matches(',').parse::<u64>() {
                    current_last_opened =
                        Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs));
                }
            } else if trimmed == "}" || trimmed == "}," {
                if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
                    let last_opened = current_last_opened.take().unwrap_or(SystemTime::now());
                    projects.push_back(RecentProject {
                        name,
                        path,
                        last_opened,
                    });
                }
            }
        }

        projects
    }

    fn extract_json_value(line: &str) -> Option<String> {
        let colon_idx = line.find(':')?;
        let after_colon = &line[colon_idx + 1..].trim();
        let first_quote = after_colon.find('"')?;
        let rest = &after_colon[first_quote + 1..];
        let last_quote = rest.rfind('"')?;
        Some(Self::unescape_json(&rest[..last_quote]))
    }

    fn escape_json(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    fn unescape_json(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('\\') => result.push('\\'),
                    Some('"') => result.push('"'),
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some(other) => result.push(other),
                    None => break,
                }
            } else {
                result.push(c);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_roundtrip() {
        let mut projects = VecDeque::new();
        projects.push_back(RecentProject::from_path(Path::new("D:\\Test\\Project1")));
        projects.push_back(RecentProject::from_path(Path::new("D:\\Test\\Project2")));

        let json = RecentProjectsManager::to_json(&projects);
        let parsed = RecentProjectsManager::parse_json(&json);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Project1");
        assert_eq!(parsed[1].name, "Project2");
    }

    #[test]
    fn test_add_and_limit() {
        let mut manager = RecentProjectsManager {
            projects: VecDeque::new(),
            config_dir: PathBuf::from("."),
            max_count: 3,
        };

        manager.add(Path::new("/path/a"));
        manager.add(Path::new("/path/b"));
        manager.add(Path::new("/path/c"));
        manager.add(Path::new("/path/d"));

        assert_eq!(manager.len(), 3);
        assert_eq!(manager.list()[0].name, "d");
        assert_eq!(manager.list()[1].name, "c");
        assert_eq!(manager.list()[2].name, "b");
    }

    #[test]
    fn test_duplicate_moves_to_front() {
        let mut manager = RecentProjectsManager {
            projects: VecDeque::new(),
            config_dir: PathBuf::from("."),
            max_count: 5,
        };

        manager.add(Path::new("/path/a"));
        manager.add(Path::new("/path/b"));
        manager.add(Path::new("/path/a")); // 重复

        assert_eq!(manager.len(), 2);
        assert_eq!(manager.list()[0].name, "a");
        assert_eq!(manager.list()[1].name, "b");
    }
}
