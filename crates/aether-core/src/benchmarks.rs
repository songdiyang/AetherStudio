use std::time::{Duration, Instant};

use crate::buffer::piece_table::PieceTable;
use crate::buffer::text_buffer::TextBuffer;
use crate::incremental_lexer::IncrementalLexer;
use crate::lexer::Language;
use crate::simd_utils::{count_newlines_simd, find_byte_simd, skip_whitespace_simd};

/// 性能测试结果
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub name: String,
    pub iterations: u64,
    pub total_time: Duration,
    pub avg_time: Duration,
    pub min_time: Duration,
    pub max_time: Duration,
    pub throughput: f64,
}

impl BenchmarkResult {
    pub fn new(name: &str, iterations: u64, times: &[Duration]) -> Self {
        let total_time: Duration = times.iter().sum();
        let avg_time = total_time / iterations as u32;
        let min_time = *times.iter().min().unwrap_or(&Duration::ZERO);
        let max_time = *times.iter().max().unwrap_or(&Duration::ZERO);
        let throughput = iterations as f64 / total_time.as_secs_f64().max(0.0001);

        Self {
            name: name.to_string(),
            iterations,
            total_time,
            avg_time,
            min_time,
            max_time,
            throughput,
        }
    }

    pub fn report(&self) -> String {
        format!(
            "{:50} | {:>6} iter | {:>8.3}ms avg | {:>8.3}ms min | {:>8.3}ms max | {:>10.0} ops/s",
            self.name,
            self.iterations,
            self.avg_time.as_secs_f64() * 1000.0,
            self.min_time.as_secs_f64() * 1000.0,
            self.max_time.as_secs_f64() * 1000.0,
            self.throughput
        )
    }
}

/// 运行基准测试，带最大时间限制
pub fn run_benchmark<F>(
    name: &str,
    iterations: u64,
    max_total_secs: u64,
    mut f: F,
) -> BenchmarkResult
where
    F: FnMut(),
{
    let max_duration = Duration::from_secs(max_total_secs);
    let start_total = Instant::now();
    let mut times = Vec::with_capacity(iterations as usize);

    // 预热（最多3次）
    for _ in 0..iterations.min(3) {
        f();
    }

    // 正式测试
    for _ in 0..iterations {
        if start_total.elapsed() > max_duration {
            break;
        }
        let start = Instant::now();
        f();
        let elapsed = start.elapsed();
        times.push(elapsed);
    }

    let actual_iterations = times.len() as u64;
    BenchmarkResult::new(name, actual_iterations, &times)
}

/// 生成测试文本数据
pub fn generate_text_lines(line_count: usize, line_length: usize) -> String {
    let mut text = String::with_capacity(line_count * (line_length + 1));
    for i in 0..line_count {
        let line = format!(
            "Line {:06}: {}",
            i,
            "x".repeat(line_length.saturating_sub(16))
        );
        text.push_str(&line);
        text.push('\n');
    }
    text
}

// ============================================================================
// PieceTable 性能测试
// ============================================================================

/// 测试 PieceTable 大文件加载性能
pub fn benchmark_piece_table_load() -> BenchmarkResult {
    let text = generate_text_lines(10_000, 100); // 1万行

    run_benchmark("PieceTable::from_string (10K lines)", 50, 5, || {
        let _pt = PieceTable::from_string(text.clone());
    })
}

/// 测试 PieceTable 从文件加载性能（使用内存映射）
pub fn benchmark_piece_table_from_file() -> BenchmarkResult {
    use std::io::Write;
    let text = generate_text_lines(100_000, 100); // 10万行
    let path = std::path::PathBuf::from("test_benchmark_file.txt");
    {
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    let result = run_benchmark("PieceTable::from_file (100K lines, ~4MB)", 20, 10, || {
        let _pt = PieceTable::from_file(&path).unwrap();
    });

    let _ = std::fs::remove_file(&path);
    result
}

/// 测试 PieceTable 单次插入性能
pub fn benchmark_piece_table_insert() -> BenchmarkResult {
    let text = generate_text_lines(5_000, 100);
    let mut pt = PieceTable::from_string(text);
    let pos = pt.len_bytes() / 2;

    run_benchmark("PieceTable::insert (5K lines)", 200, 5, || {
        pt.insert(pos, "inserted text here");
    })
}

/// 测试 PieceTable 多次插入性能（累积碎片）
pub fn benchmark_piece_table_many_inserts() -> BenchmarkResult {
    let text = generate_text_lines(500, 100);

    run_benchmark("PieceTable::insert x500 (500 lines)", 1, 10, || {
        let mut pt = PieceTable::from_string(text.clone());
        for i in 0..500 {
            let pos = pt.len_bytes() / 2;
            pt.insert(pos, &format!("insert{}", i));
        }
    })
}

/// 测试 PieceTable 删除性能
pub fn benchmark_piece_table_delete() -> BenchmarkResult {
    let text = generate_text_lines(5_000, 100);
    let mut pt = PieceTable::from_string(text);
    let len = pt.len_bytes();
    let mid = len / 2;

    run_benchmark("PieceTable::delete (5K lines)", 200, 5, || {
        pt.delete(mid, mid + 20);
    })
}

/// 测试 PieceTable 行读取性能
pub fn benchmark_piece_table_line_read() -> BenchmarkResult {
    let text = generate_text_lines(10_000, 100);
    let pt = PieceTable::from_string(text);
    let total_lines = pt.len_lines();
    let step = total_lines / 200;

    run_benchmark(
        "PieceTable::get_line (10K lines, 200 samples)",
        200,
        5,
        || {
            for i in (0..total_lines).step_by(step.max(1)) {
                let _ = pt.get_line(i);
            }
        },
    )
}

/// 测试 PieceTable 全文读取性能
pub fn benchmark_piece_table_full_text() -> BenchmarkResult {
    let text = generate_text_lines(10_000, 100);
    let pt = PieceTable::from_string(text);

    run_benchmark("PieceTable::get_all_text (10K lines)", 20, 5, || {
        let _ = pt.get_all_text();
    })
}

/// 测试 PieceTable 快照创建性能
pub fn benchmark_piece_table_snapshot() -> BenchmarkResult {
    let text = generate_text_lines(5_000, 100);
    let pt = PieceTable::from_string(text);

    run_benchmark("PieceTable::create_snapshot (5K lines)", 200, 5, || {
        let _ = pt.create_snapshot();
    })
}

/// 测试 PieceTable 编辑吞吐量（模拟实际使用场景）
pub fn benchmark_piece_table_edit_throughput() -> BenchmarkResult {
    let text = generate_text_lines(1_000, 100);
    let mut pt = PieceTable::from_string(text);
    let mut counter = 0u64;

    run_benchmark("PieceTable::edit throughput (1K lines)", 500, 5, || {
        let pos = pt.len_bytes() / 2;
        pt.insert(pos, &format!("edit{}", counter));
        counter += 1;
        if counter % 10 == 0 {
            let del_pos = pt.len_bytes() / 3;
            let len = pt.len_bytes();
            if del_pos + 5 < len {
                pt.delete(del_pos, del_pos + 5);
            }
        }
    })
}

// ============================================================================
// SIMD 加速测试
// ============================================================================

/// 测试SIMD换行符计数 vs 标量计数
pub fn benchmark_simd_newlines() -> BenchmarkResult {
    let data = generate_text_lines(10_000, 100).into_bytes();

    run_benchmark("SIMD::count_newlines (10K lines)", 100, 5, || {
        let count = count_newlines_simd(&data);
        assert!(count > 0);
    })
}

/// 测试SIMD字节查找
pub fn benchmark_simd_find_byte() -> BenchmarkResult {
    let data = generate_text_lines(10_000, 100).into_bytes();

    run_benchmark("SIMD::find_byte (10K lines)", 100, 5, || {
        let pos = find_byte_simd(&data, b'\n');
        assert!(pos.is_some());
    })
}

/// 测试SIMD跳过空白
pub fn benchmark_simd_skip_whitespace() -> BenchmarkResult {
    let data = "    \t\t    hello world".as_bytes();

    run_benchmark("SIMD::skip_whitespace", 1000, 5, || {
        let pos = skip_whitespace_simd(data, 0);
        // 4个空格 + 2个制表符 + 4个空格 = 10个空白字符
        assert_eq!(pos, 10);
    })
}

// ============================================================================
// 增量词法分析器测试
// ============================================================================

/// 测试增量lexer全量分析性能
pub fn benchmark_incremental_lexer_full() -> BenchmarkResult {
    let lines: Vec<String> = generate_text_lines(5_000, 100)
        .lines()
        .map(|s| s.to_string())
        .collect();

    run_benchmark("IncrementalLexer::analyze_all (5K lines)", 50, 5, || {
        let mut lexer = IncrementalLexer::new(Language::Rust);
        lexer.analyze_all(&lines);
    })
}

/// 测试增量lexer增量更新性能（vs 全量重新分析）
pub fn benchmark_incremental_lexer_update() -> BenchmarkResult {
    let lines: Vec<String> = generate_text_lines(5_000, 100)
        .lines()
        .map(|s| s.to_string())
        .collect();
    let mut lexer = IncrementalLexer::new(Language::Rust);
    lexer.analyze_all(&lines);

    // 模拟插入一行后的增量更新
    let mut modified_lines = lines.clone();
    modified_lines.insert(2500, "    let x = 42;".to_string());
    let edit = crate::buffer::text_buffer::EditResult::new(2500, 2500, 1);

    run_benchmark(
        "IncrementalLexer::update (5K lines, 1 edit)",
        100,
        5,
        || {
            let mut l = IncrementalLexer::new(Language::Rust);
            l.analyze_all(&lines); // 重新初始化
            l.update_for_edit(&edit, &modified_lines);
        },
    )
}

/// 对比：全量重新分析 vs 增量更新
pub fn benchmark_incremental_vs_full() -> BenchmarkResult {
    let lines: Vec<String> = generate_text_lines(10_000, 100)
        .lines()
        .map(|s| s.to_string())
        .collect();

    run_benchmark(
        "IncrementalLexer::speedup vs full (10K lines)",
        20,
        5,
        || {
            // 全量分析
            let mut lexer1 = IncrementalLexer::new(Language::Rust);
            lexer1.analyze_all(&lines);

            // 增量更新（模拟编辑后）
            let mut modified_lines = lines.clone();
            modified_lines.insert(5000, "    let test = 1;".to_string());
            let edit = crate::buffer::text_buffer::EditResult::new(5000, 5000, 1);

            let mut lexer2 = IncrementalLexer::new(Language::Rust);
            lexer2.analyze_all(&lines);
            lexer2.update_for_edit(&edit, &modified_lines);
        },
    )
}

// ============================================================================
// 综合性能测试套件
// ============================================================================

/// 运行所有性能测试
pub fn run_all_benchmarks() -> Vec<BenchmarkResult> {
    println!("========================================");
    println!("  Aether Core 性能基准测试");
    println!("========================================");
    println!();

    let mut results = Vec::new();

    println!("--- PieceTable 测试 ---");
    results.push(benchmark_piece_table_load());
    results.push(benchmark_piece_table_from_file());
    results.push(benchmark_piece_table_insert());
    results.push(benchmark_piece_table_many_inserts());
    results.push(benchmark_piece_table_delete());
    results.push(benchmark_piece_table_line_read());
    results.push(benchmark_piece_table_full_text());
    results.push(benchmark_piece_table_snapshot());
    results.push(benchmark_piece_table_edit_throughput());

    println!("--- SIMD 加速测试 ---");
    results.push(benchmark_simd_newlines());
    results.push(benchmark_simd_find_byte());
    results.push(benchmark_simd_skip_whitespace());

    println!("--- 增量词法分析器测试 ---");
    results.push(benchmark_incremental_lexer_full());
    results.push(benchmark_incremental_lexer_update());
    results.push(benchmark_incremental_vs_full());

    println!();
    println!("--- 结果汇总 ---");
    println!("{}", "=".repeat(120));
    println!(
        "{:50} | {:>6} | {:>10} | {:>10} | {:>10} | {:>10}",
        "测试名称", "迭代", "平均(ms)", "最小(ms)", "最大(ms)", "吞吐量"
    );
    println!("{}", "-".repeat(120));
    for result in &results {
        println!("{}", result.report());
    }
    println!("{}", "=".repeat(120));

    results
}
