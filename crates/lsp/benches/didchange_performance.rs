use beancount_language_server::beancount_data::BeancountData;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use ropey::Rope;
use std::sync::Arc;
use tree_sitter::Parser;
use tree_sitter_beancount::tree_sitter;

fn benchmark_didchange_scenarios(c: &mut Criterion) {
    // Sample beancount data representing a typical file
    let _sample = r#"
2024-01-01 open Assets:Checking USD
2024-01-01 open Assets:Savings USD
2024-01-01 open Expenses:Food
2024-01-01 open Expenses:Transport
2024-01-01 open Income:Salary

2024-01-02 commodity USD
2024-01-02 commodity EUR

2024-01-05 * "Grocery Store" "Weekly groceries" #food #shopping ^receipt-001
    Expenses:Food  150.00 USD
    Assets:Checking  -150.00 USD

2024-01-06 * "Gas Station" "Fuel" #transport
    Expenses:Transport  45.00 USD
    Assets:Checking  -45.00 USD

2024-01-10 * "Restaurant" "Dinner" #food
    Expenses:Food  75.00 USD
    Assets:Checking  -75.00 USD
"#;

    // Modified sample after keystroke (added one character)
    let sample_modified = r#"
2024-01-01 open Assets:Checking USD
2024-01-01 open Assets:Savings USD
2024-01-01 open Expenses:Food
2024-01-01 open Expenses:Transport
2024-01-01 open Income:Salary

2024-01-02 commodity USD
2024-01-02 commodity EUR

2024-01-05 * "Grocery Store" "Weekly groceries" #food #shopping ^receipt-001
    Expenses:Food  150.00 USD
    Assets:Checking  -150.00 USD

2024-01-06 * "Gas Station" "Fuel" #transport
    Expenses:Transport  45.00 USD
    Assets:Checking  -45.00 USD

2024-01-10 * "Restaurant" "Dinner" #food
    Expenses:Food  75.00 USD
    Assets:Checking  -75.00 USD
x
"#;

    // Benchmark: OLD approach (eager extraction on every keystroke)
    c.bench_function("didchange_old_eager_extraction", |b| {
        b.iter(|| {
            // Parse the modified content
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_beancount::language())
                .expect("Failed to set language");
            let tree = parser
                .parse(black_box(sample_modified), None)
                .expect("Failed to parse");
            let content = Rope::from_str(sample_modified);

            // OLD: Extract BeancountData immediately (expensive!)
            let tree_arc = Arc::new(tree);
            let data = BeancountData::new(&tree_arc, &content);
            black_box(Arc::new(data))
        })
    });

    // Benchmark: NEW approach (lazy extraction - just parse and cache tree)
    c.bench_function("didchange_new_lazy_extraction", |b| {
        b.iter(|| {
            // Parse the modified content
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_beancount::language())
                .expect("Failed to set language");
            let tree = parser
                .parse(black_box(sample_modified), None)
                .expect("Failed to parse");

            // NEW: Just cache the tree, no extraction (fast!)
            black_box(Arc::new(tree))
        })
    });

    // Benchmark: Extraction on-demand (when completion is requested)
    c.bench_function("extraction_on_demand", |b| {
        // Pre-parse the tree
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_beancount::language())
            .expect("Failed to set language");
        let tree = parser
            .parse(sample_modified, None)
            .expect("Failed to parse");
        let tree_arc = Arc::new(tree);
        let content = Rope::from_str(sample_modified);

        b.iter(|| {
            // Extraction happens only when completion is triggered
            let data = BeancountData::new(black_box(&tree_arc), black_box(&content));
            black_box(Arc::new(data))
        })
    });

    // Benchmark: Full cycle (parse tree only, no extraction)
    c.bench_function("didchange_parse_tree_only", |b| {
        b.iter(|| {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_beancount::language())
                .expect("Failed to set language");
            let tree = parser
                .parse(black_box(sample_modified), None)
                .expect("Failed to parse");
            black_box(tree)
        })
    });
}

criterion_group!(benches, benchmark_didchange_scenarios);
criterion_main!(benches);
