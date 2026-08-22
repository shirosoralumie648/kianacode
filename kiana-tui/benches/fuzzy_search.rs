use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use kiana_tui::search::fuzzy::{CaseMatching, FuzzyMatcher};

/// 生成测试数据集
fn generate_test_data(size: usize) -> Vec<String> {
    let patterns = vec![
        "src/components/Button.tsx",
        "src/utils/helper.ts",
        "tests/integration/test.rs",
        "docs/api/reference.md",
        "config/database.yml",
        "lib/core/engine.rs",
        "app/models/user.rb",
        "vendor/assets/style.css",
    ];

    (0..size)
        .map(|i| {
            let base = &patterns[i % patterns.len()];
            format!("{}.{}", base, i)
        })
        .collect()
}

/// 基准测试：单次匹配
fn bench_single_match(c: &mut Criterion) {
    let matcher = FuzzyMatcher::new();
    let text = "src/components/Button.tsx";

    c.bench_function("single_match_short", |b| {
        b.iter(|| matcher.match_str(black_box(text), black_box("btn")));
    });

    c.bench_function("single_match_long", |b| {
        b.iter(|| matcher.match_str(black_box(text), black_box("components")));
    });
}

/// 基准测试：批量匹配（1k, 10k 项）
fn bench_batch_match(c: &mut Criterion) {
    let matcher = FuzzyMatcher::new();

    for size in [1_000, 10_000].iter() {
        let data = generate_test_data(*size);

        c.bench_with_input(BenchmarkId::new("batch_match", size), size, |b, _| {
            b.iter(|| {
                let pattern = black_box("btn");
                let mut results: Vec<_> = data
                    .iter()
                    .filter_map(|item| matcher.match_str(item, pattern).map(|r| (r.score, item)))
                    .collect();
                results.sort_by_key(|(score, _)| -score);
                results.truncate(20);
                results
            });
        });
    }
}

/// 基准测试：不同大小写模式
fn bench_case_matching(c: &mut Criterion) {
    let text = "src/components/Button.tsx";

    let matcher_smart = FuzzyMatcher::new().with_case_matching(CaseMatching::Smart);
    let matcher_insensitive = FuzzyMatcher::new().with_case_matching(CaseMatching::Insensitive);
    let matcher_sensitive = FuzzyMatcher::new().with_case_matching(CaseMatching::Sensitive);

    c.bench_function("case_smart", |b| {
        b.iter(|| matcher_smart.match_str(black_box(text), black_box("button")));
    });

    c.bench_function("case_insensitive", |b| {
        b.iter(|| matcher_insensitive.match_str(black_box(text), black_box("BUTTON")));
    });

    c.bench_function("case_sensitive", |b| {
        b.iter(|| matcher_sensitive.match_str(black_box(text), black_box("Button")));
    });
}

/// 基准测试：不同模式长度
fn bench_pattern_length(c: &mut Criterion) {
    let matcher = FuzzyMatcher::new();
    let text = "src/components/Button.tsx";

    let patterns = vec![
        ("short_2", "bt"),
        ("short_3", "btn"),
        ("medium_6", "button"),
        ("long_10", "components"),
    ];

    for (name, pattern) in patterns {
        c.bench_function(name, |b| {
            b.iter(|| matcher.match_str(black_box(text), black_box(pattern)));
        });
    }
}

criterion_group!(
    benches,
    bench_single_match,
    bench_batch_match,
    bench_case_matching,
    bench_pattern_length
);
criterion_main!(benches);
