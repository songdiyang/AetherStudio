//! 记忆存储适配层（MemoryStore）
//!
//! 对话持久化 + ACE 上下文工程条目库的统一抽象。
//! 上层（ai_warm_data / ai_agent / reflector）只依赖 [`MemoryStore`] trait，
//! 底层实现可整体替换：
//!
//! - 当前实现：[`SqliteMemoryStore`]（VS Code/Cursor 同款方案）
//!   - rusqlite bundled（零外部依赖，编译进 exe）
//!   - WAL 模式保证崩溃安全
//!   - sqlite-vec 向量插件（vec0 虚拟表，供大模型语义检索）
//!   - 配套 [`JsonlSessionLog`]：VS Code 同款追加式会话日志（热数据）
//! - 未来可替换：Qdrant Edge / LanceDB / 云端同步实现，只需再实现一次 trait
//!
//! 设计原则（来自 ACE 论文 arXiv:2510.04618）：
//! - 条目化增量更新，禁止整体重写
//! - playbook 条目带 helpful/harmful 计数器，作为"权重"演化信号

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 默认向量维度（bge-small-zh-v1.5）
pub const DEFAULT_EMBEDDING_DIM: usize = 512;

// ============================================================================
// 数据结构
// ============================================================================

/// 会话元数据
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// 所属工作区哈希（VS Code workspaceStorage 同款绑定方式）
    pub workspace_hash: String,
    pub mode: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: u32,
}

/// 单条对话消息（Cursor bubble 模式：一条消息一行，带 schema 版本号）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub conv_id: String,
    pub msg_index: u32,
    pub role: String,
    pub content: String,
    /// 语义检索向量（写入时可先为空，后台补齐）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    pub schema_ver: u32,
    pub created_at: u64,
}

/// ACE playbook 条目（权重沉淀的最小单元）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybookBullet {
    pub id: String,
    /// 分类：tool_use / coding_style / pitfalls / project_facts ...
    pub section: String,
    pub content: String,
    /// “权重”：被引用且任务成功的次数
    pub helpful_count: u32,
    /// 被引用但产生负效果的次数
    pub harmful_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    pub created_at: u64,
    pub updated_at: u64,
}

// ============================================================================
// 适配器 trait —— 上层只依赖此接口
// ============================================================================

pub trait MemoryStore: Send + Sync {
    // ---- 会话 ----
    fn upsert_conversation(&self, conv: &Conversation) -> Result<(), String>;
    fn list_conversations(&self, limit: usize) -> Result<Vec<Conversation>, String>;
    fn delete_conversation(&self, conv_id: &str) -> Result<(), String>;
    /// 清空全部会话（级联删除消息与向量索引）；返回删除条数
    fn clear_all_conversations(&self) -> Result<usize, String> {
        let all = self.list_conversations(1_000_000)?;
        let n = all.len();
        for c in all {
            self.delete_conversation(&c.id)?;
        }
        Ok(n)
    }

    // ---- 消息 ----
    fn append_message(&self, msg: &ChatMessage) -> Result<(), String>;
    fn get_messages(&self, conv_id: &str) -> Result<Vec<ChatMessage>, String>;

    // ---- playbook 条目（ACE 权重沉淀）----
    fn upsert_bullet(&self, bullet: &PlaybookBullet) -> Result<(), String>;
    /// 条目反馈计数（helpful=true 则 helpful_count+1，否则 harmful_count+1）
    fn bullet_feedback(&self, bullet_id: &str, helpful: bool) -> Result<(), String>;
    fn list_bullets(&self, section: Option<&str>) -> Result<Vec<PlaybookBullet>, String>;

    // ---- 语义检索（sqlite-vec；实现若无向量能力可返回空）----
    fn search_messages(
        &self,
        query_embedding: &[f32],
        conv_id: Option<&str>,
        k: usize,
    ) -> Result<Vec<(ChatMessage, f32)>, String>;
    fn search_bullets(
        &self,
        query_embedding: &[f32],
        k: usize,
    ) -> Result<Vec<(PlaybookBullet, f32)>, String>;

    fn flush(&self) -> Result<(), String>;

    // ---- 混合检索（FTS5 关键词 + 向量，RRF 融合；默认退化为纯向量）----
    fn hybrid_search_messages(
        &self,
        _query_text: &str,
        query_embedding: &[f32],
        conv_id: Option<&str>,
        k: usize,
    ) -> Result<Vec<(ChatMessage, f32)>, String> {
        self.search_messages(query_embedding, conv_id, k)
    }

    // ---- 会话检索（关键词 + 工作区过滤；默认内存过滤）----
    fn search_conversations(
        &self,
        keyword: &str,
        workspace_hash: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Conversation>, String> {
        let kw = keyword.to_lowercase();
        let all = self.list_conversations(limit.max(500))?;
        Ok(all
            .into_iter()
            .filter(|c| {
                let ws_ok = workspace_hash
                    .map(|h| c.workspace_hash == h)
                    .unwrap_or(true);
                let kw_ok = kw.is_empty() || c.title.to_lowercase().contains(&kw);
                ws_ok && kw_ok
            })
            .take(limit)
            .collect())
    }

    // ---- playbook 管理 ----
    /// 更新条目内容/分类（保留计数器与 ID）
    fn update_bullet(&self, bullet: &PlaybookBullet) -> Result<(), String> {
        self.upsert_bullet(bullet)
    }
    /// 删除条目（含向量索引）
    fn delete_bullet(&self, _bullet_id: &str) -> Result<(), String> {
        Err("该存储实现不支持删除条目".to_string())
    }

    // ---- grow-and-refine 剪枝 ----
    /// 按配置剪枝高 harmful 条目，写审计日志；返回处理报告
    fn prune_bullets(&self, _config: &PruneConfig) -> Result<PruneReport, String> {
        Ok(PruneReport::default())
    }
    /// 查询剪枝审计日志
    fn list_prune_log(&self, _limit: usize) -> Result<Vec<PruneLogEntry>, String> {
        Ok(Vec::new())
    }
}

// ============================================================================
// grow-and-refine 剪枝配置与审计
// ============================================================================

/// 剪枝配置（阈值均可调）
#[derive(Clone, Debug)]
pub struct PruneConfig {
    /// harmful_count 达到该值才考虑剪枝
    pub harmful_threshold: i64,
    /// helpful + harmful 总数达到该值才评估（避免新条目被误删）
    pub min_total_uses: i64,
    /// 试运行：只返回候选，不实际删除、不写日志
    pub dry_run: bool,
}

impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            harmful_threshold: 3,
            min_total_uses: 5,
            dry_run: false,
        }
    }
}

/// 剪枝报告
#[derive(Clone, Debug, Default)]
pub struct PruneReport {
    /// 实际删除的条目数（dry_run 时为候选数）
    pub pruned: usize,
    pub bullet_ids: Vec<String>,
}

/// 剪枝审计日志条目
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PruneLogEntry {
    pub id: String,
    pub bullet_id: String,
    pub content: String,
    pub helpful_count: i64,
    pub harmful_count: i64,
    pub reason: String,
    pub pruned_at: u64,
}

// ============================================================================
// SQLite 实现（VS Code/Cursor 同款 + sqlite-vec 向量插件）
// ============================================================================

/// 基于 rusqlite(bundled) + sqlite-vec 的本地嵌入式实现
///
/// 单文件库，WAL 模式。向量索引存于 vec0 虚拟表，
/// rowid 与 messages / playbook_bullets 主键一一对应。
pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,
    embedding_dim: usize,
    path: PathBuf,
}

impl SqliteMemoryStore {
    /// 打开或创建数据库（dir 不存在会自动创建）
    pub fn open(dir: &Path, embedding_dim: usize) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
        let path = dir.join("aether_memory.db");

        // 注册 sqlite-vec 为自动扩展（静态链接进进程，所有连接生效）
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open(&path).map_err(|e| format!("打开数据库失败: {}", e))?;

        // VS Code state.vscdb 同款工程配置
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -16000;",
        )
        .map_err(|e| format!("PRAGMA 配置失败: {}", e))?;

        let store = Self {
            conn: Mutex::new(conn),
            embedding_dim,
            path,
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let schema = format!(
            "CREATE TABLE IF NOT EXISTS conversations (
                id             TEXT PRIMARY KEY,
                title          TEXT NOT NULL DEFAULT '',
                workspace_hash TEXT NOT NULL DEFAULT '',
                mode           TEXT NOT NULL DEFAULT '',
                created_at     INTEGER NOT NULL,
                updated_at     INTEGER NOT NULL,
                message_count  INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS messages (
                rowid      INTEGER PRIMARY KEY AUTOINCREMENT,
                id         TEXT NOT NULL UNIQUE,
                conv_id    TEXT NOT NULL,
                msg_index  INTEGER NOT NULL,
                role       TEXT NOT NULL,
                content    TEXT NOT NULL,
                schema_ver INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conv
                ON messages(conv_id, msg_index);

            CREATE TABLE IF NOT EXISTS playbook_bullets (
                rowid         INTEGER PRIMARY KEY AUTOINCREMENT,
                id            TEXT NOT NULL UNIQUE,
                section       TEXT NOT NULL,
                content       TEXT NOT NULL,
                helpful_count INTEGER NOT NULL DEFAULT 0,
                harmful_count INTEGER NOT NULL DEFAULT 0,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bullets_section
                ON playbook_bullets(section);

            CREATE VIRTUAL TABLE IF NOT EXISTS vec_messages USING vec0(
                embedding float[{dim}]
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_bullets USING vec0(
                embedding float[{dim}]
            );

            -- FTS5 全文索引（trigram 分词：中英文子串匹配均可）
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                content='messages',
                content_rowid='rowid',
                tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS messages_fts_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_fts_ad AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_fts_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
                INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
            END;

            -- 剪枝审计日志（grow-and-refine）
            CREATE TABLE IF NOT EXISTS prune_log (
                id            TEXT PRIMARY KEY,
                bullet_id     TEXT NOT NULL,
                content       TEXT NOT NULL,
                helpful_count INTEGER NOT NULL,
                harmful_count INTEGER NOT NULL,
                reason        TEXT NOT NULL,
                pruned_at     INTEGER NOT NULL
            );",
            dim = self.embedding_dim
        );
        conn.execute_batch(&schema)
            .map_err(|e| format!("初始化 schema 失败: {}", e))
    }

    /// 数据库文件路径
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn check_dim(&self, embedding: &[f32]) -> Result<(), String> {
        if embedding.len() != self.embedding_dim {
            return Err(format!(
                "向量维度不匹配: 期望 {}, 实际 {}",
                self.embedding_dim,
                embedding.len()
            ));
        }
        Ok(())
    }

    /// 向量检索公共逻辑：查 vec0 表取 rowid + distance，再回表取实体
    fn vec_knn(
        conn: &Connection,
        vec_table: &str,
        query_embedding: &[f32],
        k: usize,
    ) -> Result<Vec<(i64, f32)>, String> {
        let sql = format!(
            "SELECT rowid, distance FROM {} WHERE embedding MATCH ? ORDER BY distance LIMIT ?",
            vec_table
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("向量查询准备失败: {}", e))?;
        let rows = stmt
            .query_map(params![f32_to_bytes(query_embedding), k as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f32>(1)?))
            })
            .map_err(|e| format!("向量查询失败: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn upsert_conversation(&self, conv: &Conversation) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conversations (id, title, workspace_hash, mode, created_at, updated_at, message_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                updated_at = excluded.updated_at,
                message_count = excluded.message_count",
            params![
                conv.id,
                conv.title,
                conv.workspace_hash,
                conv.mode,
                conv.created_at as i64,
                conv.updated_at as i64,
                conv.message_count as i64,
            ],
        )
        .map_err(|e| format!("写入会话失败: {}", e))?;
        Ok(())
    }

    fn list_conversations(&self, limit: usize) -> Result<Vec<Conversation>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, workspace_hash, mode, created_at, updated_at, message_count
                 FROM conversations ORDER BY updated_at DESC LIMIT ?",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    workspace_hash: row.get(2)?,
                    mode: row.get(3)?,
                    created_at: row.get::<_, i64>(4)? as u64,
                    updated_at: row.get::<_, i64>(5)? as u64,
                    message_count: row.get::<_, i64>(6)? as u32,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    fn delete_conversation(&self, conv_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        // 先删向量索引（vec0 不支持外键）
        conn.execute(
            "DELETE FROM vec_messages WHERE rowid IN (SELECT rowid FROM messages WHERE conv_id = ?)",
            params![conv_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM messages WHERE conv_id = ?", params![conv_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM conversations WHERE id = ?", params![conv_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn append_message(&self, msg: &ChatMessage) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, conv_id, msg_index, role, content, schema_ver, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                msg.id,
                msg.conv_id,
                msg.msg_index as i64,
                msg.role,
                msg.content,
                msg.schema_ver as i64,
                msg.created_at as i64,
            ],
        )
        .map_err(|e| format!("写入消息失败: {}", e))?;

        // 有向量则写入 vec0 索引（rowid 与 messages 一致）
        if let Some(emb) = &msg.embedding {
            self.check_dim(emb)?;
            let rowid: i64 = conn
                .query_row(
                    "SELECT rowid FROM messages WHERE id = ?",
                    params![msg.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO vec_messages (rowid, embedding) VALUES (?, ?)",
                params![rowid, f32_to_bytes(emb)],
            )
            .map_err(|e| format!("写入消息向量失败: {}", e))?;
        }
        Ok(())
    }

    fn get_messages(&self, conv_id: &str) -> Result<Vec<ChatMessage>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, conv_id, msg_index, role, content, schema_ver, created_at
                 FROM messages WHERE conv_id = ? ORDER BY msg_index ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![conv_id], |row| {
                Ok(ChatMessage {
                    id: row.get(0)?,
                    conv_id: row.get(1)?,
                    msg_index: row.get::<_, i64>(2)? as u32,
                    role: row.get(3)?,
                    content: row.get(4)?,
                    embedding: None,
                    schema_ver: row.get::<_, i64>(5)? as u32,
                    created_at: row.get::<_, i64>(6)? as u64,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    fn upsert_bullet(&self, bullet: &PlaybookBullet) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO playbook_bullets (id, section, content, helpful_count, harmful_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                section = excluded.section,
                content = excluded.content,
                updated_at = excluded.updated_at",
            params![
                bullet.id,
                bullet.section,
                bullet.content,
                bullet.helpful_count as i64,
                bullet.harmful_count as i64,
                bullet.created_at as i64,
                bullet.updated_at as i64,
            ],
        )
        .map_err(|e| format!("写入条目失败: {}", e))?;

        if let Some(emb) = &bullet.embedding {
            self.check_dim(emb)?;
            let rowid: i64 = conn
                .query_row(
                    "SELECT rowid FROM playbook_bullets WHERE id = ?",
                    params![bullet.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO vec_bullets (rowid, embedding) VALUES (?, ?)",
                params![rowid, f32_to_bytes(emb)],
            )
            .map_err(|e| format!("写入条目向量失败: {}", e))?;
        }
        Ok(())
    }

    fn bullet_feedback(&self, bullet_id: &str, helpful: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let field = if helpful {
            "helpful_count"
        } else {
            "harmful_count"
        };
        let sql = format!(
            "UPDATE playbook_bullets SET {} = {} + 1 WHERE id = ?",
            field, field
        );
        conn.execute(&sql, params![bullet_id])
            .map_err(|e| format!("条目反馈失败: {}", e))?;
        Ok(())
    }

    fn list_bullets(&self, section: Option<&str>) -> Result<Vec<PlaybookBullet>, String> {
        let conn = self.conn.lock().unwrap();
        let (sql, param): (String, Option<String>) = match section {
            Some(s) => (
                "SELECT id, section, content, helpful_count, harmful_count, created_at, updated_at
                 FROM playbook_bullets WHERE section = ? ORDER BY updated_at DESC"
                    .to_string(),
                Some(s.to_string()),
            ),
            None => (
                "SELECT id, section, content, helpful_count, harmful_count, created_at, updated_at
                 FROM playbook_bullets ORDER BY updated_at DESC"
                    .to_string(),
                None,
            ),
        };
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<PlaybookBullet> {
            Ok(PlaybookBullet {
                id: row.get(0)?,
                section: row.get(1)?,
                content: row.get(2)?,
                helpful_count: row.get::<_, i64>(3)? as u32,
                harmful_count: row.get::<_, i64>(4)? as u32,
                embedding: None,
                created_at: row.get::<_, i64>(5)? as u64,
                updated_at: row.get::<_, i64>(6)? as u64,
            })
        };
        let collected: Result<Vec<_>, _> = match param {
            Some(p) => stmt
                .query_map(params![p], map_row)
                .map_err(|e| e.to_string())?
                .collect(),
            None => stmt
                .query_map([], map_row)
                .map_err(|e| e.to_string())?
                .collect(),
        };
        collected.map_err(|e| e.to_string())
    }

    fn search_messages(
        &self,
        query_embedding: &[f32],
        conv_id: Option<&str>,
        k: usize,
    ) -> Result<Vec<(ChatMessage, f32)>, String> {
        self.check_dim(query_embedding)?;
        let conn = self.conn.lock().unwrap();
        // 先向量取候选（放大 4 倍以容纳 conv 过滤），再回表过滤
        let candidates = Self::vec_knn(&conn, "vec_messages", query_embedding, k * 4)?;
        let mut results = Vec::new();
        for (rowid, distance) in candidates {
            let msg: Result<ChatMessage, _> = conn.query_row(
                "SELECT id, conv_id, msg_index, role, content, schema_ver, created_at
                 FROM messages WHERE rowid = ?",
                params![rowid],
                |row| {
                    Ok(ChatMessage {
                        id: row.get(0)?,
                        conv_id: row.get(1)?,
                        msg_index: row.get::<_, i64>(2)? as u32,
                        role: row.get(3)?,
                        content: row.get(4)?,
                        embedding: None,
                        schema_ver: row.get::<_, i64>(5)? as u32,
                        created_at: row.get::<_, i64>(6)? as u64,
                    })
                },
            );
            if let Ok(m) = msg {
                if let Some(cid) = conv_id {
                    if m.conv_id != cid {
                        continue;
                    }
                }
                results.push((m, distance));
                if results.len() >= k {
                    break;
                }
            }
        }
        Ok(results)
    }

    fn search_bullets(
        &self,
        query_embedding: &[f32],
        k: usize,
    ) -> Result<Vec<(PlaybookBullet, f32)>, String> {
        self.check_dim(query_embedding)?;
        let conn = self.conn.lock().unwrap();
        let candidates = Self::vec_knn(&conn, "vec_bullets", query_embedding, k)?;
        let mut results = Vec::new();
        for (rowid, distance) in candidates {
            let bullet: Result<PlaybookBullet, _> = conn.query_row(
                "SELECT id, section, content, helpful_count, harmful_count, created_at, updated_at
                 FROM playbook_bullets WHERE rowid = ?",
                params![rowid],
                |row| {
                    Ok(PlaybookBullet {
                        id: row.get(0)?,
                        section: row.get(1)?,
                        content: row.get(2)?,
                        helpful_count: row.get::<_, i64>(3)? as u32,
                        harmful_count: row.get::<_, i64>(4)? as u32,
                        embedding: None,
                        created_at: row.get::<_, i64>(5)? as u64,
                        updated_at: row.get::<_, i64>(6)? as u64,
                    })
                },
            );
            if let Ok(b) = bullet {
                results.push((b, distance));
            }
        }
        Ok(results)
    }

    fn flush(&self) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(|e| format!("WAL checkpoint 失败: {}", e))
    }

    fn hybrid_search_messages(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        conv_id: Option<&str>,
        k: usize,
    ) -> Result<Vec<(ChatMessage, f32)>, String> {
        self.check_dim(query_embedding)?;
        const RRF_K: f32 = 60.0; // RRF 平滑常数（论文常用值）
        let conn = self.conn.lock().unwrap();

        // 1. FTS5 候选（bm25 升序 = 更相关；trigram 需 ≥3 字符）
        let mut fts_rank: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        if query_text.chars().count() >= 3 {
            let fts_query = format!("\"{}\"", query_text.replace('"', " "));
            let mut stmt = conn
                .prepare(
                    "SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?
                     ORDER BY bm25(messages_fts) LIMIT ?",
                )
                .map_err(|e| format!("FTS 查询准备失败: {}", e))?;
            let rows = stmt
                .query_map(params![fts_query, (k * 4) as i64], |r| r.get::<_, i64>(0))
                .map_err(|e| format!("FTS 查询失败: {}", e))?;
            for (i, rid) in rows.flatten().enumerate() {
                fts_rank.insert(rid, i + 1);
            }
        }

        // 2. 向量候选
        let vec_candidates = Self::vec_knn(&conn, "vec_messages", query_embedding, k * 4)?;

        // 3. RRF 融合：score = Σ 1/(60 + rank)
        let mut scores: std::collections::HashMap<i64, f32> = std::collections::HashMap::new();
        for (rid, rank) in &fts_rank {
            *scores.entry(*rid).or_default() += 1.0 / (RRF_K + *rank as f32);
        }
        for (i, (rid, _)) in vec_candidates.iter().enumerate() {
            *scores.entry(*rid).or_default() += 1.0 / (RRF_K + (i + 1) as f32);
        }

        // 4. 按融合分降序，回表取实体并做 conv 过滤
        let mut ranked: Vec<(i64, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut results = Vec::new();
        for (rowid, score) in ranked {
            let msg: Result<ChatMessage, _> = conn.query_row(
                "SELECT id, conv_id, msg_index, role, content, schema_ver, created_at
                 FROM messages WHERE rowid = ?",
                params![rowid],
                |row| {
                    Ok(ChatMessage {
                        id: row.get(0)?,
                        conv_id: row.get(1)?,
                        msg_index: row.get::<_, i64>(2)? as u32,
                        role: row.get(3)?,
                        content: row.get(4)?,
                        embedding: None,
                        schema_ver: row.get::<_, i64>(5)? as u32,
                        created_at: row.get::<_, i64>(6)? as u64,
                    })
                },
            );
            if let Ok(m) = msg {
                if let Some(cid) = conv_id {
                    if m.conv_id != cid {
                        continue;
                    }
                }
                results.push((m, score));
                if results.len() >= k {
                    break;
                }
            }
        }
        Ok(results)
    }

    fn search_conversations(
        &self,
        keyword: &str,
        workspace_hash: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Conversation>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, workspace_hash, mode, created_at, updated_at, message_count
                 FROM conversations
                 WHERE (?1 IS NULL OR workspace_hash = ?1)
                   AND (?2 = '' OR title LIKE '%' || ?2 || '%')
                 ORDER BY updated_at DESC LIMIT ?3",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![workspace_hash, keyword, limit as i64], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    workspace_hash: row.get(2)?,
                    mode: row.get(3)?,
                    created_at: row.get::<_, i64>(4)? as u64,
                    updated_at: row.get::<_, i64>(5)? as u64,
                    message_count: row.get::<_, i64>(6)? as u32,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    fn delete_bullet(&self, bullet_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM vec_bullets WHERE rowid IN (SELECT rowid FROM playbook_bullets WHERE id = ?)",
            params![bullet_id],
        )
        .map_err(|e| format!("删除条目向量失败: {}", e))?;
        let n = conn
            .execute(
                "DELETE FROM playbook_bullets WHERE id = ?",
                params![bullet_id],
            )
            .map_err(|e| format!("删除条目失败: {}", e))?;
        if n == 0 {
            return Err(format!("条目不存在: {}", bullet_id));
        }
        Ok(())
    }

    fn prune_bullets(&self, config: &PruneConfig) -> Result<PruneReport, String> {
        let conn = self.conn.lock().unwrap();
        // 候选：使用量足够 + harmful 达阈值 + harmful 超过 helpful
        let mut stmt = conn
            .prepare(
                "SELECT id, content, helpful_count, harmful_count FROM playbook_bullets
                 WHERE (helpful_count + harmful_count) >= ?1
                   AND harmful_count >= ?2
                   AND harmful_count > helpful_count",
            )
            .map_err(|e| e.to_string())?;
        let candidates: Vec<(String, String, i64, i64)> = stmt
            .query_map(
                params![config.min_total_uses, config.harmful_threshold],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut report = PruneReport::default();
        for (id, content, helpful, harmful) in candidates {
            report.pruned += 1;
            report.bullet_ids.push(id.clone());
            if config.dry_run {
                continue;
            }
            // 审计日志（先写日志再删除，保证可追溯）
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            conn.execute(
                "INSERT INTO prune_log (id, bullet_id, content, helpful_count, harmful_count, reason, pruned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    new_id("prune"),
                    id,
                    content,
                    helpful,
                    harmful,
                    format!("harmful({}) > helpful({}) 且超过阈值", harmful, helpful),
                    now as i64,
                ],
            )
            .map_err(|e| format!("写剪枝日志失败: {}", e))?;
            // 删除条目与向量索引
            conn.execute(
                "DELETE FROM vec_bullets WHERE rowid IN (SELECT rowid FROM playbook_bullets WHERE id = ?)",
                params![id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM playbook_bullets WHERE id = ?", params![id])
                .map_err(|e| e.to_string())?;
        }
        Ok(report)
    }

    fn list_prune_log(&self, limit: usize) -> Result<Vec<PruneLogEntry>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, bullet_id, content, helpful_count, harmful_count, reason, pruned_at
                 FROM prune_log ORDER BY pruned_at DESC LIMIT ?",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(PruneLogEntry {
                    id: row.get(0)?,
                    bullet_id: row.get(1)?,
                    content: row.get(2)?,
                    helpful_count: row.get(3)?,
                    harmful_count: row.get(4)?,
                    reason: row.get(5)?,
                    pruned_at: row.get::<_, i64>(6)? as u64,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}

// ============================================================================
// JSONL 会话日志（VS Code 同款热数据：每会话一个追加式文件）
// ============================================================================

/// VS Code chatSessions 同款：活跃会话的追加式 JSONL 日志。
/// 温数据归档（写入 SQLite）成功后，对应日志文件可删除。
pub struct JsonlSessionLog {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl JsonlSessionLog {
    /// 打开（或创建）某个会话的日志文件：dir/session_<id>.jsonl
    pub fn open(dir: &Path, session_id: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
        let path = dir.join(format!("session_{}.jsonl", session_id));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("打开会话日志失败: {}", e))?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    /// 追加一条消息（一行 JSON + 立即 flush，崩溃最多丢最后一行）
    pub fn append(&self, msg: &ChatMessage) -> Result<(), String> {
        use std::io::Write;
        let line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        let mut file = self.file.lock().unwrap();
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.flush())
            .map_err(|e| format!("写入会话日志失败: {}", e))
    }

    /// 重放日志，重建全部消息（VS Code mutation-replay 同款思路）
    pub fn read_all(&self) -> Result<Vec<ChatMessage>, String> {
        let content = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<ChatMessage>(line) {
                out.push(msg);
            }
            // 损坏行跳过：追加写被中断时最后一行可能不完整
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ============================================================================
// 工具函数
// ============================================================================

/// f32 数组 → little-endian 字节（sqlite-vec 的 float32 blob 格式）
fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// 生成短唯一 ID（时间戳 + 进程内原子计数，无外部依赖）
pub fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!(
        "{}-{:x}-{:x}",
        prefix,
        ts,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (PathBuf, SqliteMemoryStore) {
        let dir = std::env::temp_dir().join(format!("aether_mem_test_{}", new_id("d")));
        let store = SqliteMemoryStore::open(&dir, 4).unwrap();
        (dir, store)
    }

    fn sample_msg(conv_id: &str, idx: u32, content: &str, emb: Option<Vec<f32>>) -> ChatMessage {
        ChatMessage {
            id: new_id("m"),
            conv_id: conv_id.to_string(),
            msg_index: idx,
            role: if idx % 2 == 0 { "user" } else { "assistant" }.into(),
            content: content.into(),
            embedding: emb,
            schema_ver: 1,
            created_at: 1700000000 + idx as u64,
        }
    }

    #[test]
    fn test_conversation_and_messages() {
        let (dir, store) = temp_store();
        store
            .upsert_conversation(&Conversation {
                id: "c1".into(),
                title: "测试会话".into(),
                workspace_hash: "abc".into(),
                mode: "chat".into(),
                created_at: 1,
                updated_at: 2,
                message_count: 2,
            })
            .unwrap();
        store
            .append_message(&sample_msg("c1", 0, "你好", None))
            .unwrap();
        store
            .append_message(&sample_msg("c1", 1, "你好！有什么可以帮你？", None))
            .unwrap();

        let msgs = store.get_messages("c1").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "你好");

        let convs = store.list_conversations(10).unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].title, "测试会话");

        store.delete_conversation("c1").unwrap();
        assert!(store.get_messages("c1").unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clear_all_conversations() {
        let (dir, store) = temp_store();
        for i in 0..3 {
            let cid = format!("c{}", i);
            store
                .upsert_conversation(&Conversation {
                    id: cid.clone(),
                    title: format!("会话{}", i),
                    workspace_hash: "ws".into(),
                    mode: "Agent".into(),
                    created_at: 1,
                    updated_at: 2 + i as u64,
                    message_count: 1,
                })
                .unwrap();
            store
                .append_message(&sample_msg(&cid, 0, "你好", None))
                .unwrap();
        }
        assert_eq!(store.list_conversations(10).unwrap().len(), 3);

        let n = store.clear_all_conversations().unwrap();
        assert_eq!(n, 3);
        assert!(store.list_conversations(10).unwrap().is_empty());
        // 级联删除消息
        assert!(store.get_messages("c0").unwrap().is_empty());
        // 空库清空返回 0
        assert_eq!(store.clear_all_conversations().unwrap(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_vector_search() {
        let (dir, store) = temp_store();
        // 维度=4 便于测试
        store
            .append_message(&sample_msg("c1", 0, "a", Some(vec![1.0, 0.0, 0.0, 0.0])))
            .unwrap();
        store
            .append_message(&sample_msg("c1", 1, "b", Some(vec![0.0, 1.0, 0.0, 0.0])))
            .unwrap();
        store
            .append_message(&sample_msg("c1", 2, "c", Some(vec![0.9, 0.1, 0.0, 0.0])))
            .unwrap();

        let results = store
            .search_messages(&[1.0, 0.0, 0.0, 0.0], None, 2)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.content, "a"); // 最近的应该是完全相同的向量
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_playbook_bullets() {
        let (dir, store) = temp_store();
        store
            .upsert_bullet(&PlaybookBullet {
                id: "b1".into(),
                section: "tool_use".into(),
                content: "git 操作前先检查工作区是否干净".into(),
                helpful_count: 0,
                harmful_count: 0,
                embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store.bullet_feedback("b1", true).unwrap();
        store.bullet_feedback("b1", true).unwrap();
        store.bullet_feedback("b1", false).unwrap();

        let bullets = store.list_bullets(Some("tool_use")).unwrap();
        assert_eq!(bullets.len(), 1);
        assert_eq!(bullets[0].helpful_count, 2);
        assert_eq!(bullets[0].harmful_count, 1);

        let hits = store.search_bullets(&[1.0, 0.0, 0.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.id, "b1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_jsonl_session_log() {
        let dir = std::env::temp_dir().join(format!("aether_jsonl_test_{}", new_id("d")));
        let log = JsonlSessionLog::open(&dir, "s1").unwrap();
        log.append(&sample_msg("s1", 0, "第一条", None)).unwrap();
        log.append(&sample_msg("s1", 1, "第二条", None)).unwrap();

        let msgs = log.read_all().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "第二条");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_hybrid_search_keyword_hit() {
        let (dir, store) = temp_store();
        // 关键词可命中的消息（向量故意设成无关方向）
        store
            .append_message(&sample_msg(
                "c1",
                0,
                "如何配置 rust-analyzer 的 LSP 服务",
                Some(vec![0.0, 1.0, 0.0, 0.0]),
            ))
            .unwrap();
        store
            .append_message(&sample_msg(
                "c1",
                1,
                "完全不相关的闲聊内容",
                Some(vec![0.0, 0.0, 1.0, 0.0]),
            ))
            .unwrap();

        // 查询向量指向第三个方向（两条都不近），关键词命中第一条
        let hits = store
            .hybrid_search_messages("rust-analyzer", &[1.0, 0.0, 0.0, 0.0], None, 5)
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0.content, "如何配置 rust-analyzer 的 LSP 服务");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_search_conversations_filter() {
        let (dir, store) = temp_store();
        for (id, title, ws) in [
            ("c1", "LSP 配置讨论", "ws-a"),
            ("c2", "数据库设计", "ws-a"),
            ("c3", "LSP 性能优化", "ws-b"),
        ] {
            store
                .upsert_conversation(&Conversation {
                    id: id.into(),
                    title: title.into(),
                    workspace_hash: ws.into(),
                    mode: "chat".into(),
                    created_at: 1,
                    updated_at: 2,
                    message_count: 0,
                })
                .unwrap();
        }
        // 工作区过滤
        let r = store.search_conversations("", Some("ws-a"), 10).unwrap();
        assert_eq!(r.len(), 2);
        // 关键词 + 工作区
        let r = store.search_conversations("LSP", Some("ws-a"), 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "c1");
        // 仅关键词
        let r = store.search_conversations("LSP", None, 10).unwrap();
        assert_eq!(r.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_bullet() {
        let (dir, store) = temp_store();
        store
            .upsert_bullet(&PlaybookBullet {
                id: "b1".into(),
                section: "s".into(),
                content: "待删除".into(),
                helpful_count: 0,
                harmful_count: 0,
                embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        store.delete_bullet("b1").unwrap();
        assert!(store.list_bullets(None).unwrap().is_empty());
        // 向量索引同步删除
        assert!(store
            .search_bullets(&[1.0, 0.0, 0.0, 0.0], 5)
            .unwrap()
            .is_empty());
        assert!(store.delete_bullet("b1").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_prune_bullets_with_audit() {
        let (dir, store) = temp_store();
        // 高 harmful 条目：应被剪枝
        store
            .upsert_bullet(&PlaybookBullet {
                id: "bad".into(),
                section: "s".into(),
                content: "总是出错的策略".into(),
                helpful_count: 1,
                harmful_count: 5,
                embedding: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        // 正常条目：保留
        store
            .upsert_bullet(&PlaybookBullet {
                id: "good".into(),
                section: "s".into(),
                content: "有效的策略".into(),
                helpful_count: 8,
                harmful_count: 1,
                embedding: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        // 使用量不足的新条目：即使 harmful>helpful 也保留（min_total_uses 保护）
        store
            .upsert_bullet(&PlaybookBullet {
                id: "new".into(),
                section: "s".into(),
                content: "新条目".into(),
                helpful_count: 0,
                harmful_count: 3,
                embedding: None,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

        let cfg = PruneConfig {
            harmful_threshold: 3,
            min_total_uses: 5,
            dry_run: false,
        };
        // dry_run 不实际删除
        let dry = store
            .prune_bullets(&PruneConfig {
                dry_run: true,
                ..cfg.clone()
            })
            .unwrap();
        assert_eq!(dry.pruned, 1);
        assert_eq!(store.list_bullets(None).unwrap().len(), 3);

        let report = store.prune_bullets(&cfg).unwrap();
        assert_eq!(report.pruned, 1);
        assert_eq!(report.bullet_ids, vec!["bad".to_string()]);
        let remaining = store.list_bullets(None).unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|b| b.id != "bad"));

        // 审计日志
        let log = store.list_prune_log(10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].bullet_id, "bad");
        assert_eq!(log[0].harmful_count, 5);
        assert!(log[0].reason.contains("harmful"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
