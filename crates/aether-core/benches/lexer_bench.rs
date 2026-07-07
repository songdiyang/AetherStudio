use aether_core::lexer::Language;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn rust_sample() -> &'static str {
    // 约 3KB 的 Rust 代码样本，包含关键字、字符串、数字、注释、生命周期等常见 token
    r##"
use std::collections::HashMap;

/// 文档注释示例
pub struct Foo<T> {
    value: T,
    map: HashMap<String, i32>,
}

impl<T: Clone> Foo<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            map: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&i32> {
        self.map.get(key)
    }

    pub fn set(&mut self, key: String, value: i32) {
        self.map.insert(key, value);
    }
}

fn main() {
    let mut foo = Foo::new(42);
    foo.set("answer".to_string(), 42);
    let range = 0..100;
    let sum: i32 = range.map(|x| x * 2).sum();
    println!("sum = {}", sum);
    let s = r#"raw string"#;
    let c = 'c';
    let lifetime = &'static str;
}
"##
}

fn js_sample() -> &'static str {
    // 约 2KB 的 JS/TS 代码样本
    r##"
import { useState, useEffect } from 'react';

export function Counter({ initial = 0 }) {
    const [count, setCount] = useState(initial);

    useEffect(() => {
        const id = setInterval(() => {
            setCount(c => c + 1);
        }, 1000);
        return () => clearInterval(id);
    }, []);

    const doubled = count * 2;
    const regex = /foo|bar/gi;
    const template = `Count: ${count}, doubled: ${doubled}`;

    return (
        <button onClick={() => setCount(n => n + 1)}>
            {template}
        </button>
    );
}
"##
}

fn python_sample() -> &'static str {
    // 约 2KB 的 Python 代码样本
    r##"
from typing import List, Dict, Optional

class Parser:
    def __init__(self, tokens: List[str]):
        self.tokens = tokens
        self.pos = 0

    def peek(self) -> Optional[str]:
        if self.pos < len(self.tokens):
            return self.tokens[self.pos]
        return None

    def consume(self) -> str:
        tok = self.tokens[self.pos]
        self.pos += 1
        return tok

    def parse_number(self) -> float:
        return float(self.consume())

    def parse_list(self) -> List[float]:
        result = []
        while self.peek() is not None:
            result.append(self.parse_number())
        return result
"##
}

fn c_sample() -> &'static str {
    // 约 2KB 的 C 代码样本
    r##"
#include <stdio.h>
#include <stdlib.h>

#define MAX_SIZE 1024

typedef struct {
    int x;
    int y;
} Point;

Point* create_point(int x, int y) {
    Point* p = malloc(sizeof(Point));
    if (!p) return NULL;
    p->x = x;
    p->y = y;
    return p;
}

int main(void) {
    Point* p = create_point(10, 20);
    if (p) {
        printf("point: (%d, %d)\n", p->x, p->y);
        free(p);
    }
    return 0;
}
"##
}

fn bench_lexer(c: &mut Criterion) {
    let mut group = c.benchmark_group("lexer");

    let samples: Vec<(&str, Language, &str)> = vec![
        ("rust", Language::Rust, rust_sample()),
        ("js", Language::JavaScript, js_sample()),
        ("python", Language::Python, python_sample()),
        ("c", Language::C, c_sample()),
    ];

    for (name, lang, text) in &samples {
        let len = text.len() as u64;
        group.throughput(Throughput::Bytes(len));
        group.bench_with_input(BenchmarkId::new(*name, len), text, |b, text| {
            b.iter(|| {
                let tokens = lang.lex_full(text);
                black_box(tokens);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_lexer);
criterion_main!(benches);
