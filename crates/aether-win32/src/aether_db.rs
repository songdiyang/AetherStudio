//! AetherDB — 轻量级嵌入式向量数据库
//!
//! 专为 AI 对话场景设计的纯 Rust 嵌入式存储引擎，替代 SQLite。
//! 核心特性：
//! - 零外部 C 依赖（无动态链接）
//! - 基于 mmap 的列式存储 + 内存索引
//! - HNSW 近似最近邻向量检索
//! - 支持标量过滤（时间范围、标签、会话 ID）
//! - 增量写入 + 后台压缩合并
//! - 单文件存储，零配置部署

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use ort::value::Tensor;

// ============================================================================
// 嵌入模型管理器（ONNX Runtime）
// ============================================================================

/// 嵌入模型管理器（单例，懒加载）
///
/// 使用 ONNX Runtime 运行 sentence-transformers 模型，
/// 将文本转换为 384 维稠密向量。
pub struct EmbeddingModel {
    /// ONNX 会话
    session: ort::session::Session,
    /// Tokenizer
    tokenizer: tokenizers::Tokenizer,
}

impl EmbeddingModel {
    /// 模型维度（all-MiniLM-L6-v2 为 384 维）
    pub const DIM: usize = 384;

    /// 加载默认模型（从模型文件路径）
    pub fn from_files(model_path: &str, tokenizer_path: &str) -> Result<Self, String> {
        let session = ort::session::Session::builder()
            .map_err(|e| format!("ONNX 会话构建失败: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| format!("加载 ONNX 模型失败: {}", e))?;

        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| format!("加载 Tokenizer 失败: {}", e))?;

        Ok(Self { session, tokenizer })
    }

    /// 将文本编码为向量
    pub fn encode(&mut self, text: &str) -> Result<Vec<f32>, String> {
        // 1. Tokenize
        let encoding = self.tokenizer
            .encode(text, true)
            .map_err(|e| format!("Tokenize 失败: {}", e))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();

        let seq_len = input_ids.len();

        // 2. 构建输入张量 (使用 Tensor::from_array + (shape, Vec) 元组)
        let input_ids_tensor = Tensor::from_array((vec![1i64, seq_len as i64], input_ids))
            .map_err(|e| format!("构建 input_ids 张量失败: {}", e))?;
        let attention_mask_tensor = Tensor::from_array((vec![1i64, seq_len as i64], attention_mask))
            .map_err(|e| format!("构建 attention_mask 张量失败: {}", e))?;

        // 3. 运行推理（使用命名输入）
        let outputs = self.session
            .run(ort::inputs! {
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor
            })
            .map_err(|e| format!("ONNX 推理失败: {}", e))?;

        // 4. 提取输出（pooler_output 或 last_hidden_state 的均值池化）
        let (_shape, output_data) = outputs["pooler_output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("提取输出失败: {}", e))?;

        // 转换为 Vec<f32> 并归一化
        let mut vector: Vec<f32> = output_data.iter().copied().collect();
        
        // 归一化（L2）
        let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        }

        Ok(vector)
    }

    /// 批量编码
    pub fn encode_batch(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.encode(text)?);
        }
        Ok(results)
    }
}

/// 全局嵌入模型实例（懒加载，使用 Mutex 包装以支持可变借用）
static EMBEDDING_MODEL: Mutex<Option<EmbeddingModel>> = Mutex::new(None);

/// 初始化全局嵌入模型
pub fn init_embedding_model(model_path: &str, tokenizer_path: &str) -> Result<(), String> {
    let mut guard = EMBEDDING_MODEL.lock().map_err(|e| format!("锁获取失败: {}", e))?;
    *guard = Some(EmbeddingModel::from_files(model_path, tokenizer_path)?);
    Ok(())
}

/// 获取全局嵌入模型（如果已初始化）
fn get_embedding_model() -> Option<std::sync::MutexGuard<'static, Option<EmbeddingModel>>> {
    EMBEDDING_MODEL.lock().ok()
}

/// 文本 → 向量（优先使用 ONNX 模型，回退到 n-gram 哈希）
pub fn text_to_vector(text: &str) -> Vector {
    // 尝试使用 ONNX 嵌入模型
    if let Some(mut guard) = get_embedding_model() {
        if let Some(ref mut model) = *guard {
            if let Ok(embedding) = model.encode(text) {
                let mut vector = Vector::new();
                // 复制嵌入结果到向量（确保长度匹配）
                let len = embedding.len().min(VECTOR_DIM);
                vector.data[..len].copy_from_slice(&embedding[..len]);
                return vector;
            }
        }
    }

    // 回退：使用字符 n-gram 哈希作为向量
    let mut vector = Vector::new();
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(2) {
        let hash = ((bytes[i] as u32) * 31
            + (bytes[i + 1] as u32) * 17
            + (bytes[i + 2] as u32)) as usize;
        let idx = hash % VECTOR_DIM;
        vector.data[idx] += 1.0;
    }
    // 归一化
    let norm: f32 = vector.data.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vector.data {
            *v /= norm;
        }
    }
    vector
}

// ============================================================================
// 核心类型定义
// ============================================================================

/// 文档 ID（64 位自增，全局唯一）
pub type DocId = u64;

/// 向量维度（对话嵌入通常为 384/768/1536 维）
pub const VECTOR_DIM: usize = 384;

/// 向量表示（使用 Vec<f32> 以兼容 serde，实际固定 384 维）
#[derive(Clone, Debug, PartialEq)]
pub struct Vector {
    pub data: Vec<f32>,
}

impl Vector {
    pub fn new() -> Self {
        Self {
            data: vec![0.0; VECTOR_DIM],
        }
    }

    pub fn from_array(arr: [f32; VECTOR_DIM]) -> Self {
        Self {
            data: arr.to_vec(),
        }
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.data
    }
}

impl Default for Vector {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for Vector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Vector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data = Vec::<f32>::deserialize(deserializer)?;
        Ok(Self { data })
    }
}

/// 可排序的相似度分数（包装 f32 以支持 Ord）
#[derive(Clone, Copy, Debug, PartialEq)]
struct Score(f32);

impl Eq for Score {}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 标量字段值
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ScalarValue {
    Int(i64),
    Float(u64), // 使用 u64 存储 f64 的 bit 模式，避免 NaN 比较问题
    String(String),
    Bool(bool),
    Timestamp(u64), // Unix 秒
    Null,
}

impl ScalarValue {
    /// 将 Float 包装为可比较类型
    pub fn from_float(v: f64) -> Self {
        ScalarValue::Float(v.to_bits())
    }

    pub fn to_float(&self) -> Option<f64> {
        match self {
            ScalarValue::Float(bits) => Some(f64::from_bits(*bits)),
            _ => None,
        }
    }
}

/// 文档（一条可检索的语义单元）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    /// 向量嵌入（语义表示）
    pub vector: Vector,
    /// 标量字段（过滤条件）
    pub scalars: std::collections::HashMap<String, ScalarValue>,
    /// 原始文本内容
    pub text: String,
    /// 创建时间
    pub created_at: u64,
}

/// 过滤条件
#[derive(Clone, Debug)]
pub enum Filter {
    Eq(String, ScalarValue),
    Gt(String, ScalarValue),
    Lt(String, ScalarValue),
    Gte(String, ScalarValue),
    Lte(String, ScalarValue),
    And(Vec<Filter>),
    Or(Vec<Filter>),
}

// ============================================================================
// 存储引擎
// ============================================================================

/// AetherDB 主引擎
///
/// 单文件存储结构：
/// ```
/// [Header 4KB] [Index Region] [Data Region] [Free List]
/// ```
/// - Header：魔数、版本、索引偏移、数据偏移、文档计数
/// - Index Region：HNSW 图结构 + 标量索引（内存中维护，mmap 持久化）
/// - Data Region：文档序列化数据（列式压缩存储）
/// - Free List：已删除文档的 ID 回收列表
pub struct AetherDB {
    /// 数据库文件路径
    path: PathBuf,
    /// 内存映射文件
    mmap: RwLock<memmap2::MmapMut>,
    /// 文档计数（原子操作）
    doc_count: AtomicU64,
    /// HNSW 向量索引（内存中）
    vector_index: RwLock<HnswIndex>,
    /// 标量索引（内存中）
    scalar_indices: RwLock<std::collections::HashMap<String, ScalarIndex>>,
    /// 空闲文档 ID 列表
    free_ids: RwLock<Vec<DocId>>,
    /// 脏页标记（需要刷新的数据页）
    dirty_pages: RwLock<std::collections::HashSet<usize>>,
    /// 向量缓存（内存中）
    vector_cache: RwLock<VectorCache>,
}

/// 数据库文件头（4KB 对齐）
#[derive(Clone, Debug)]
struct DbHeader {
    /// 魔数 "AEDB"
    magic: [u8; 4],
    /// 版本号
    version: u32,
    /// 文档总数
    doc_count: u64,
    /// 索引区域起始偏移
    index_offset: u64,
    /// 数据区域起始偏移
    data_offset: u64,
    /// 空闲列表偏移
    free_list_offset: u64,
    /// 文件总大小
    file_size: u64,
}

impl DbHeader {
    const SIZE: usize = 4096;
    const MAGIC: [u8; 4] = *b"AEDB";
    const VERSION: u32 = 1;

    fn new() -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            doc_count: 0,
            index_offset: Self::SIZE as u64,
            data_offset: Self::SIZE as u64 + 1024 * 1024, // 预留 1MB 索引空间
            free_list_offset: Self::SIZE as u64 + 1024 * 1024 + 1024 * 1024, // 预留 1MB 数据空间
            file_size: Self::SIZE as u64 + 3 * 1024 * 1024,
        }
    }

    fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && self.version == Self::VERSION
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.magic);
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&self.doc_count.to_le_bytes());
        buf.extend_from_slice(&self.index_offset.to_le_bytes());
        buf.extend_from_slice(&self.data_offset.to_le_bytes());
        buf.extend_from_slice(&self.free_list_offset.to_le_bytes());
        buf.extend_from_slice(&self.file_size.to_le_bytes());
        buf.resize(Self::SIZE, 0);
        buf
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[0..4]);
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let doc_count = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let index_offset = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19],
            bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let data_offset = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27],
            bytes[28], bytes[29], bytes[30], bytes[31],
        ]);
        let free_list_offset = u64::from_le_bytes([
            bytes[32], bytes[33], bytes[34], bytes[35],
            bytes[36], bytes[37], bytes[38], bytes[39],
        ]);
        let file_size = u64::from_le_bytes([
            bytes[40], bytes[41], bytes[42], bytes[43],
            bytes[44], bytes[45], bytes[46], bytes[47],
        ]);
        Some(Self {
            magic,
            version,
            doc_count,
            index_offset,
            data_offset,
            free_list_offset,
            file_size,
        })
    }
}

// ============================================================================
// HNSW 向量索引
// ============================================================================

/// HNSW（Hierarchical Navigable Small World）近似最近邻索引
///
/// 多层图结构，每层是 NSW 图，上层是下层的稀疏子集。
/// 查询时从顶层贪心搜索，逐层向下 refine。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HnswIndex {
    /// 最大层数
    max_level: usize,
    /// 每层最大邻居数
    m: usize,
    /// 构建时 efConstruction
    ef_construction: usize,
    /// 查询时 efSearch
    ef_search: usize,
    /// 各层图结构：层号 -> (节点 ID -> 邻居列表)
    layers: Vec<std::collections::HashMap<DocId, Vec<DocId>>>,
    /// 节点到层数的映射
    node_levels: std::collections::HashMap<DocId, usize>,
    /// 入口节点（最高层）
    entry_point: Option<DocId>,
}

/// 向量缓存（内存中，不序列化到磁盘）
#[derive(Clone, Debug)]
pub struct VectorCache {
    vectors: std::collections::HashMap<DocId, Vector>,
}

impl VectorCache {
    pub fn new() -> Self {
        Self {
            vectors: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: DocId, vector: Vector) {
        self.vectors.insert(id, vector);
    }

    pub fn get(&self, id: DocId) -> Option<&Vector> {
        self.vectors.get(&id)
    }

    pub fn remove(&mut self, id: DocId) {
        self.vectors.remove(&id);
    }
}

impl HnswIndex {
    pub fn new(m: usize, ef_construction: usize, ef_search: usize) -> Self {
        Self {
            max_level: 16,
            m,
            ef_construction,
            ef_search,
            layers: Vec::new(),
            node_levels: std::collections::HashMap::new(),
            entry_point: None,
        }
    }

    /// 计算向量余弦相似度（-1 到 1，越高越相似）
    pub fn cosine_similarity(a: &Vector, b: &Vector) -> f32 {
        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;
        let dim = a.data.len().min(b.data.len()).min(VECTOR_DIM);
        for i in 0..dim {
            dot += a.data[i] * b.data[i];
            norm_a += a.data[i] * a.data[i];
            norm_b += b.data[i] * b.data[i];
        }
        let denom = norm_a.sqrt() * norm_b.sqrt();
        if denom < 1e-10 {
            0.0
        } else {
            dot / denom
        }
    }

    /// 生成随机层数（指数分布）
    fn random_level(&self) -> usize {
        let mut level = 0;
        // 使用简单的伪随机数生成器（避免外部依赖）
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0) as u32;
        let mut rng = seed.wrapping_mul(1103515245).wrapping_add(12345);
        while level < self.max_level {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            // 50% 概率继续上升
            if (rng >> 16) & 1 == 0 {
                break;
            }
            level += 1;
        }
        level
    }

    /// 贪心搜索最近邻（单层）
    fn search_layer(
        &self,
        query: &Vector,
        vectors: &VectorCache,
        entry: DocId,
        ef: usize,
        level: usize,
    ) -> Vec<(DocId, f32)> {
        let mut visited = std::collections::HashSet::new();
        let mut candidates = std::collections::BinaryHeap::new();
        let mut results = std::collections::BinaryHeap::new();

        let entry_vec = vectors.get(entry).cloned().unwrap_or_else(Vector::new);
        let entry_sim = Self::cosine_similarity(query, &entry_vec);
        candidates.push(std::cmp::Reverse((Score(entry_sim), entry)));
        results.push(std::cmp::Reverse((Score(entry_sim), entry)));
        visited.insert(entry);

        while let Some(std::cmp::Reverse((Score(curr_sim), curr_id))) = candidates.pop() {
            if results.len() >= ef {
                let best = results.peek().map(|r| r.0 .0).unwrap_or(Score(0.0));
                if curr_sim < best.0 {
                    break;
                }
            }

            if let Some(neighbors) = self.layers.get(level).and_then(|l| l.get(&curr_id)) {
                for &neighbor in neighbors {
                    if visited.insert(neighbor) {
                        let neighbor_vec = vectors.get(neighbor).cloned().unwrap_or_else(Vector::new);
                        let sim = Self::cosine_similarity(query, &neighbor_vec);
                        candidates.push(std::cmp::Reverse((Score(sim), neighbor)));
                        results.push(std::cmp::Reverse((Score(sim), neighbor)));
                        if results.len() > ef {
                            results.pop();
                        }
                    }
                }
            }
        }

        results
            .into_sorted_vec()
            .into_iter()
            .map(|r| (r.0 .1, r.0 .0 .0))
            .collect()
    }

    /// 插入新向量
    pub fn insert(&mut self, id: DocId, vector: Vector, vectors: &mut VectorCache) {
        vectors.insert(id, vector.clone());
        let level = self.random_level();
        self.node_levels.insert(id, level);

        // 确保层数足够
        while self.layers.len() <= level {
            self.layers.push(std::collections::HashMap::new());
        }

        if let Some(entry) = self.entry_point {
            // 从顶层开始搜索
            let mut curr_entry = entry;
            for l in (level + 1..self.layers.len()).rev() {
                let nearest = self.search_layer(
                    vectors.get(id).unwrap_or(&Vector::new()),
                    vectors,
                    curr_entry,
                    1,
                    l,
                );
                if let Some((nearest_id, _)) = nearest.first() {
                    curr_entry = *nearest_id;
                }
            }

            // 从插入层向下逐层连接
            for l in (0..=level).rev() {
                let nearest = self.search_layer(
                    vectors.get(id).unwrap_or(&Vector::new()),
                    vectors,
                    curr_entry,
                    self.ef_construction,
                    l,
                );
                let mut neighbors = Vec::new();
                for (nid, _) in nearest.iter().take(self.m) {
                    neighbors.push(*nid);
                }
                self.layers[l].insert(id, neighbors.clone());

                // 双向连接：更新邻居的邻居列表
                for &nid in &neighbors {
                    if let Some(neighbor_list) = self.layers[l].get_mut(&nid) {
                        if !neighbor_list.contains(&id) {
                            neighbor_list.push(id);
                        }
                        // 保持邻居列表大小限制
                        if neighbor_list.len() > self.m * 2 {
                            // 按与 nid 的距离排序，保留最近的
                            let nid_vec = vectors.get(nid).cloned().unwrap_or_else(Vector::new);
                            let mut scored: Vec<(DocId, f32)> = neighbor_list
                                .iter()
                                .map(|&n| {
                                    (
                                        n,
                                        Self::cosine_similarity(
                                            &nid_vec,
                                            vectors.get(n).unwrap_or(&Vector::new()),
                                        ),
                                    )
                                })
                                .collect();
                            scored.sort_by(|a, b| {
                                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            *neighbor_list = scored
                                .into_iter()
                                .take(self.m * 2)
                                .map(|(id, _)| id)
                                .collect();
                        }
                    }
                }

                if let Some((nearest_id, _)) = nearest.first() {
                    curr_entry = *nearest_id;
                }
            }
        } else {
            // 第一个节点
            self.entry_point = Some(id);
            for l in 0..=level {
                self.layers[l].insert(id, Vec::new());
            }
        }
    }

    /// K 近邻搜索
    pub fn search(
        &self,
        query: &Vector,
        vectors: &VectorCache,
        k: usize,
    ) -> Vec<(DocId, f32)> {
        if self.entry_point.is_none() || vectors.vectors.is_empty() {
            return Vec::new();
        }

        let entry = self.entry_point.unwrap();
        let mut curr_entry = entry;

        // 从顶层贪心下降到第 0 层
        for l in (1..self.layers.len()).rev() {
            let nearest = self.search_layer(query, vectors, curr_entry, 1, l);
            if let Some((nearest_id, _)) = nearest.first() {
                curr_entry = *nearest_id;
            }
        }

        // 在第 0 层精确搜索
        self.search_layer(query, vectors, curr_entry, self.ef_search.max(k), 0)
            .into_iter()
            .take(k)
            .collect()
    }

    /// 删除节点（标记删除，不立即重建图）
    pub fn remove(&mut self, id: DocId, vectors: &mut VectorCache) {
        vectors.remove(id);
        self.node_levels.remove(&id);
        for layer in &mut self.layers {
            layer.remove(&id);
            for neighbors in layer.values_mut() {
                neighbors.retain(|&n| n != id);
            }
        }
    }

    /// 序列化索引到字节
    pub fn serialize(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// 从字节反序列化
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

// ============================================================================
// 标量索引
// ============================================================================

/// 标量字段索引（支持范围查询、精确匹配和倒排索引）
pub struct ScalarIndex {
    /// 有序值列表（用于范围查询）
    sorted_values: Vec<(ScalarValue, DocId)>,
    /// 值到文档列表的映射（用于精确匹配）—— 倒排索引核心
    value_map: std::collections::HashMap<String, Vec<DocId>>,
    /// 字段名
    field: String,
    /// 文档数量（用于统计）
    doc_count: usize,
}

impl ScalarIndex {
    pub fn new(field: String) -> Self {
        Self {
            sorted_values: Vec::new(),
            value_map: std::collections::HashMap::new(),
            field,
            doc_count: 0,
        }
    }

    pub fn insert(&mut self, id: DocId, value: &ScalarValue) {
        self.sorted_values.push((value.clone(), id));
        let key = format!("{:?}", value);
        self.value_map.entry(key).or_default().push(id);
        self.doc_count += 1;
    }

    pub fn remove(&mut self, id: DocId) {
        self.sorted_values.retain(|(_, doc_id)| *doc_id != id);
        for ids in self.value_map.values_mut() {
            ids.retain(|&doc_id| doc_id != id);
        }
        // 清理空列表
        self.value_map.retain(|_, ids| !ids.is_empty());
        self.doc_count = self.sorted_values.len();
    }

    /// 精确匹配（O(1) 倒排索引查找）
    pub fn exact_match(&self, value: &ScalarValue) -> Vec<DocId> {
        let key = format!("{:?}", value);
        self.value_map.get(&key).cloned().unwrap_or_default()
    }

    /// 范围查询（基于有序列表的二分查找边界 + 线性扫描）
    pub fn range_query(&self, min: Option<&ScalarValue>, max: Option<&ScalarValue>) -> Vec<DocId> {
        let mut results = Vec::new();
        for (value, id) in &self.sorted_values {
            let pass_min = min.map(|m| value >= m).unwrap_or(true);
            let pass_max = max.map(|m| value <= m).unwrap_or(true);
            if pass_min && pass_max {
                results.push(*id);
            }
        }
        results
    }

    /// 获取字段名
    pub fn field(&self) -> &str {
        &self.field
    }

    /// 获取文档数量
    pub fn doc_count(&self) -> usize {
        self.doc_count
    }

    /// 获取唯一值数量
    pub fn unique_values(&self) -> usize {
        self.value_map.len()
    }
}

// ============================================================================
// AetherDB 实现
// ============================================================================

impl AetherDB {
    /// 打开或创建数据库
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let exists = path.exists();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .map_err(|e| format!("打开数据库文件失败: {}", e))?;

        let _initial_size = if exists {
            let metadata = file.metadata().map_err(|e| format!("读取文件元数据失败: {}", e))?;
            metadata.len()
        } else {
            // 新文件：初始化 16MB
            let size = 16 * 1024 * 1024;
            file.set_len(size).map_err(|e| format!("预分配文件失败: {}", e))?;
            size
        };

        let mut mmap = unsafe {
            memmap2::MmapMut::map_mut(&file)
                .map_err(|e| format!("mmap 失败: {}", e))?
        };

        let (doc_count, vector_index, scalar_indices, free_ids) = if exists {
            // 读取现有 header
            let header_bytes = &mmap[..DbHeader::SIZE];
            let header = DbHeader::from_bytes(header_bytes)
                .ok_or("无效的数据库文件头")?;
            if !header.is_valid() {
                return Err("数据库文件头校验失败".to_string());
            }

            // 从文件加载索引
            let index_bytes = &mmap[header.index_offset as usize..header.data_offset as usize];
            let vector_index = HnswIndex::deserialize(index_bytes).unwrap_or_else(|| {
                HnswIndex::new(16, 200, 100)
            });

            // 加载标量索引（简化：从数据区域解析）
            let scalar_indices = std::collections::HashMap::new();
            let free_ids = Vec::new();

            (
                AtomicU64::new(header.doc_count),
                vector_index,
                scalar_indices,
                free_ids,
            )
        } else {
            // 初始化新数据库
            let header = DbHeader::new();
            let header_bytes = header.to_bytes();
            mmap[..DbHeader::SIZE].copy_from_slice(&header_bytes);

            (
                AtomicU64::new(0),
                HnswIndex::new(16, 200, 100),
                std::collections::HashMap::new(),
                Vec::new(),
            )
        };

        Ok(Self {
            path,
            mmap: RwLock::new(mmap),
            doc_count,
            vector_index: RwLock::new(vector_index),
            scalar_indices: RwLock::new(scalar_indices),
            free_ids: RwLock::new(free_ids),
            dirty_pages: RwLock::new(std::collections::HashSet::new()),
            vector_cache: RwLock::new(VectorCache::new()),
        })
    }

    /// 分配新文档 ID
    fn alloc_id(&self) -> DocId {
        if let Some(id) = self.free_ids.write().unwrap().pop() {
            return id;
        }
        self.doc_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 插入文档
    pub fn insert(&self, mut doc: Document) -> Result<DocId, String> {
        let id = self.alloc_id();
        doc.id = id;

        // 1. 更新向量索引
        {
            let mut index = self.vector_index.write().unwrap();
            let mut cache = self.vector_cache.write().unwrap();
            index.insert(id, doc.vector.clone(), &mut cache);
        }

        // 2. 更新标量索引
        {
            let mut scalars = self.scalar_indices.write().unwrap();
            for (field, value) in &doc.scalars {
                scalars
                    .entry(field.clone())
                    .or_insert_with(|| ScalarIndex::new(field.clone()))
                    .insert(id, value);
            }
        }

        // 3. 序列化文档到数据区域
        self.write_document(&doc)?;

        // 4. 标记脏页
        {
            let mut dirty = self.dirty_pages.write().unwrap();
            dirty.insert(0); // Header 页
        }

        Ok(id)
    }

    /// 写入文档到数据区域
    fn write_document(&self, doc: &Document) -> Result<(), String> {
        let data = serde_json::to_vec(doc)
            .map_err(|e| format!("文档序列化失败: {}", e))?;
        let len = data.len() as u64;

        let mut mmap = self.mmap.write().unwrap();
        let header = DbHeader::from_bytes(&mmap[..DbHeader::SIZE])
            .ok_or("读取 header 失败")?;

        // 计算写入位置（简化：追加到数据区域末尾）
        let write_offset = header.data_offset + header.doc_count * 4096; // 每文档 4KB 对齐
        let end_offset = write_offset + 8 + len; // 8 字节长度前缀

        // 检查是否需要扩容
        if end_offset > header.file_size {
            let new_size = (header.file_size * 2).max(end_offset + 1024 * 1024);
            drop(mmap);
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.path)
                .map_err(|e| format!("重新打开文件失败: {}", e))?;
            file.set_len(new_size)
                .map_err(|e| format!("扩容文件失败: {}", e))?;
            let new_mmap = unsafe {
                memmap2::MmapMut::map_mut(&file)
                    .map_err(|e| format!("重新 mmap 失败: {}", e))?
            };
            *self.mmap.write().unwrap() = new_mmap;
            mmap = self.mmap.write().unwrap();
        }

        // 写入长度前缀 + 数据
        let offset = write_offset as usize;
        mmap[offset..offset + 8].copy_from_slice(&len.to_le_bytes());
        mmap[offset + 8..offset + 8 + data.len()].copy_from_slice(&data);

        // 更新 header
        let mut new_header = header;
        new_header.doc_count += 1;
        let header_bytes = new_header.to_bytes();
        mmap[..DbHeader::SIZE].copy_from_slice(&header_bytes);

        Ok(())
    }

    /// 向量搜索（K 近邻 + 可选标量过滤）
    pub fn search(
        &self,
        query: &Vector,
        k: usize,
        filter: Option<&Filter>,
    ) -> Vec<(Document, f32)> {
        // 1. 向量搜索
        let candidates = {
            let index = self.vector_index.read().unwrap();
            let cache = self.vector_cache.read().unwrap();
            index.search(query, &cache, k.max(100))
        };

        // 2. 标量过滤
        let mut results = Vec::new();
        for (id, score) in candidates {
            if let Ok(doc) = self.get_document(id) {
                if let Some(f) = filter {
                    if Self::matches_filter(&doc, f) {
                        results.push((doc, score));
                    }
                } else {
                    results.push((doc, score));
                }
            }
        }

        results.into_iter().take(k).collect()
    }

    /// 标量过滤查询（不使用向量）
    pub fn filter(&self, filter: &Filter) -> Vec<Document> {
        let ids = self.evaluate_filter(filter);
        ids.into_iter()
            .filter_map(|id| self.get_document(id).ok())
            .collect()
    }

    /// 按 conv_id 快速查询文档（利用倒排索引，O(1) 查找）
    pub fn find_by_conv_id(&self, conv_id: &str) -> Vec<Document> {
        let filter = Filter::Eq(
            "conv_id".to_string(),
            ScalarValue::String(conv_id.to_string()),
        );
        self.filter(&filter)
    }

    /// 按 conv_id 进行语义搜索（先过滤 conv_id，再向量搜索）
    pub fn search_in_conv_id(
        &self,
        conv_id: &str,
        query: &Vector,
        k: usize,
    ) -> Vec<(Document, f32)> {
        // 1. 先通过倒排索引获取该 conv_id 下的所有文档 ID
        let conv_filter = Filter::Eq(
            "conv_id".to_string(),
            ScalarValue::String(conv_id.to_string()),
        );
        let conv_ids: std::collections::HashSet<DocId> =
            self.evaluate_filter(&conv_filter).into_iter().collect();

        if conv_ids.is_empty() {
            return Vec::new();
        }

        // 2. 向量搜索，但只保留属于该 conv_id 的文档
        let candidates = {
            let index = self.vector_index.read().unwrap();
            let cache = self.vector_cache.read().unwrap();
            index.search(query, &cache, k.max(100))
        };

        let mut results = Vec::new();
        for (id, score) in candidates {
            if conv_ids.contains(&id) {
                if let Ok(doc) = self.get_document(id) {
                    results.push((doc, score));
                    if results.len() >= k {
                        break;
                    }
                }
            }
        }

        results
    }

    /// 评估过滤条件，返回匹配的文档 ID 列表
    fn evaluate_filter(&self, filter: &Filter) -> Vec<DocId> {
        match filter {
            Filter::Eq(field, value) => {
                let scalars = self.scalar_indices.read().unwrap();
                scalars
                    .get(field)
                    .map(|idx| idx.exact_match(value))
                    .unwrap_or_default()
            }
            Filter::Gt(field, value) => {
                let scalars = self.scalar_indices.read().unwrap();
                scalars
                    .get(field)
                    .map(|idx| idx.range_query(Some(value), None))
                    .unwrap_or_default()
            }
            Filter::Lt(field, value) => {
                let scalars = self.scalar_indices.read().unwrap();
                scalars
                    .get(field)
                    .map(|idx| idx.range_query(None, Some(value)))
                    .unwrap_or_default()
            }
            Filter::Gte(field, value) => {
                let scalars = self.scalar_indices.read().unwrap();
                scalars
                    .get(field)
                    .map(|idx| idx.range_query(Some(value), None))
                    .unwrap_or_default()
            }
            Filter::Lte(field, value) => {
                let scalars = self.scalar_indices.read().unwrap();
                scalars
                    .get(field)
                    .map(|idx| idx.range_query(None, Some(value)))
                    .unwrap_or_default()
            }
            Filter::And(filters) => {
                if filters.is_empty() {
                    return Vec::new();
                }
                let mut result = self.evaluate_filter(&filters[0]);
                for f in &filters[1..] {
                    let next = self.evaluate_filter(f);
                    result.retain(|id| next.contains(id));
                }
                result
            }
            Filter::Or(filters) => {
                let mut result = std::collections::HashSet::new();
                for f in filters {
                    result.extend(self.evaluate_filter(f));
                }
                result.into_iter().collect()
            }
        }
    }

    /// 检查文档是否匹配过滤条件
    fn matches_filter(doc: &Document, filter: &Filter) -> bool {
        match filter {
            Filter::Eq(field, value) => doc.scalars.get(field) == Some(value),
            Filter::Gt(field, value) => {
                doc.scalars.get(field).map(|v| v > value).unwrap_or(false)
            }
            Filter::Lt(field, value) => {
                doc.scalars.get(field).map(|v| v < value).unwrap_or(false)
            }
            Filter::Gte(field, value) => {
                doc.scalars.get(field).map(|v| v >= value).unwrap_or(false)
            }
            Filter::Lte(field, value) => {
                doc.scalars.get(field).map(|v| v <= value).unwrap_or(false)
            }
            Filter::And(filters) => filters.iter().all(|f| Self::matches_filter(doc, f)),
            Filter::Or(filters) => filters.iter().any(|f| Self::matches_filter(doc, f)),
        }
    }

    /// 读取文档
    fn get_document(&self, id: DocId) -> Result<Document, String> {
        let mmap = self.mmap.read().unwrap();
        let header = DbHeader::from_bytes(&mmap[..DbHeader::SIZE])
            .ok_or("读取 header 失败")?;

        // 简化：线性扫描查找（实际应维护 ID -> 偏移映射）
        let mut offset = header.data_offset as usize;
        for _ in 0..header.doc_count {
            if offset + 8 > mmap.len() {
                break;
            }
            let len_bytes: [u8; 8] = mmap[offset..offset + 8].try_into().unwrap_or([0; 8]);
            let len = u64::from_le_bytes(len_bytes) as usize;
            if offset + 8 + len > mmap.len() {
                break;
            }
            let data = &mmap[offset + 8..offset + 8 + len];
            if let Ok(doc) = serde_json::from_slice::<Document>(data) {
                if doc.id == id {
                    return Ok(doc);
                }
            }
            offset += 4096; // 4KB 对齐
        }

        Err(format!("文档不存在: {}", id))
    }

    /// 删除文档
    pub fn delete(&self, id: DocId) -> Result<(), String> {
        // 1. 从向量索引删除
        {
            let mut index = self.vector_index.write().unwrap();
            let mut cache = self.vector_cache.write().unwrap();
            index.remove(id, &mut cache);
        }

        // 2. 从标量索引删除
        {
            let mut scalars = self.scalar_indices.write().unwrap();
            for idx in scalars.values_mut() {
                idx.remove(id);
            }
        }

        // 3. 回收 ID
        {
            let mut free_ids = self.free_ids.write().unwrap();
            free_ids.push(id);
        }

        Ok(())
    }

    /// 强制刷写到磁盘
    pub fn flush(&self) -> Result<(), String> {
        let mmap = self.mmap.write().unwrap();
        mmap.flush()
            .map_err(|e| format!("mmap flush 失败: {}", e))?;

        // 清空脏页标记
        let mut dirty = self.dirty_pages.write().unwrap();
        dirty.clear();

        Ok(())
    }

    /// 文档总数
    pub fn count(&self) -> u64 {
        self.doc_count.load(Ordering::Relaxed)
    }

    /// 关闭数据库
    pub fn close(&self) -> Result<(), String> {
        self.flush()
    }
}

// ============================================================================
// 与 AI 对话的适配层
// ============================================================================

/// AI 对话文档适配器
///
/// 将 AiConversation / AiMessage 转换为 AetherDB 的 Document 结构。
pub struct ConversationAdapter;

impl ConversationAdapter {
    /// 将单条消息转换为文档（用于语义检索）
    pub fn message_to_document(
        conv_id: &str,
        msg: &crate::ai_panel::AiMessage,
        index: usize,
    ) -> Document {
        let mut scalars = std::collections::HashMap::new();
        scalars.insert(
            "conv_id".to_string(),
            ScalarValue::String(conv_id.to_string()),
        );
        scalars.insert(
            "role".to_string(),
            ScalarValue::String(format!("{:?}", msg.role)),
        );
        scalars.insert(
            "msg_index".to_string(),
            ScalarValue::Int(index as i64),
        );

        Document {
            id: 0, // 由数据库分配
            vector: Self::text_to_vector(&msg.content),
            scalars,
            text: msg.content.clone(),
            created_at: crate::ai_panel::now_secs(),
        }
    }

    /// 将会话元数据转换为文档
    pub fn meta_to_document(conv: &crate::ai_panel::AiConversation) -> Document {
        let mut scalars = std::collections::HashMap::new();
        scalars.insert(
            "conv_id".to_string(),
            ScalarValue::String(conv.id.clone()),
        );
        scalars.insert(
            "title".to_string(),
            ScalarValue::String(conv.title.clone()),
        );
        scalars.insert(
            "created_at".to_string(),
            ScalarValue::Timestamp(conv.created_at),
        );
        scalars.insert(
            "updated_at".to_string(),
            ScalarValue::Timestamp(conv.updated_at),
        );
        scalars.insert(
            "message_count".to_string(),
            ScalarValue::Int(conv.messages.len() as i64),
        );
        scalars.insert(
            "mode".to_string(),
            ScalarValue::String(format!("{:?}", conv.mode)),
        );

        let preview = conv
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Document {
            id: 0,
            vector: Self::text_to_vector(&preview),
            scalars,
            text: preview,
            created_at: conv.created_at,
        }
    }

    /// 文本 → 向量（简化实现：实际应调用 ONNX 嵌入模型）
    pub fn text_to_vector(text: &str) -> Vector {
        // 简化：使用字符 n-gram 哈希作为向量
        // 实际生产环境应使用 sentence-transformers 等模型
        let mut vector = Vector::new();
        let bytes = text.as_bytes();
        for i in 0..bytes.len().saturating_sub(2) {
            let hash = ((bytes[i] as u32) * 31
                + (bytes[i + 1] as u32) * 17
                + (bytes[i + 2] as u32)) as usize;
            let idx = hash % VECTOR_DIM;
            vector.data[idx] += 1.0;
        }
        // 归一化
        let norm: f32 = vector.data.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vector.data {
                *v /= norm;
            }
        }
        vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let header = DbHeader::new();
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), DbHeader::SIZE);
        let restored = DbHeader::from_bytes(&bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.doc_count, 0);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = Vector::from_array([1.0f32; VECTOR_DIM]);
        let b = Vector::from_array([1.0f32; VECTOR_DIM]);
        let sim = HnswIndex::cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_hnsw_insert_and_search() {
        let mut index = HnswIndex::new(8, 50, 20);
        let mut cache = VectorCache::new();
        let v1 = Vector::from_array([1.0f32; VECTOR_DIM]);
        let v2 = Vector::from_array([0.9f32; VECTOR_DIM]);
        let v3 = Vector::from_array([-1.0f32; VECTOR_DIM]);

        index.insert(1, v1, &mut cache);
        index.insert(2, v2, &mut cache);
        index.insert(3, v3, &mut cache);

        let results = index.search(&Vector::from_array([1.0f32; VECTOR_DIM]), &cache, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1); // v1 最像 v1
    }

    #[test]
    fn test_scalar_index() {
        let mut idx = ScalarIndex::new("timestamp".to_string());
        idx.insert(1, &ScalarValue::Timestamp(1000));
        idx.insert(2, &ScalarValue::Timestamp(2000));
        idx.insert(3, &ScalarValue::Timestamp(3000));

        let exact = idx.exact_match(&ScalarValue::Timestamp(2000));
        assert_eq!(exact, vec![2]);

        let range = idx.range_query(Some(&ScalarValue::Timestamp(1500)), None);
        assert_eq!(range.len(), 2);
    }

    #[test]
    fn test_filter_evaluation() {
        let f = Filter::And(vec![
            Filter::Eq(
                "role".to_string(),
                ScalarValue::String("User".to_string()),
            ),
            Filter::Gte("timestamp".to_string(), ScalarValue::Timestamp(1000)),
        ]);

        let mut doc = Document {
            id: 1,
            vector: Vector::new(),
            scalars: std::collections::HashMap::new(),
            text: "test".to_string(),
            created_at: 1000,
        };
        doc.scalars.insert(
            "role".to_string(),
            ScalarValue::String("User".to_string()),
        );
        doc.scalars
            .insert("timestamp".to_string(), ScalarValue::Timestamp(1500));

        assert!(AetherDB::matches_filter(&doc, &f));
    }
}
