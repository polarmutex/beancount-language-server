use beancount_language_server::beancount_data::BeancountData;
use criterion::{Criterion, criterion_group, criterion_main};
use ropey::Rope;
use std::collections::HashMap;
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use tree_sitter::Parser;
use tree_sitter_beancount::tree_sitter;

fn benchmark_data_access_patterns(c: &mut Criterion) {
    // Sample beancount data
    let sample = r#"
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

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_beancount::language())
        .expect("Failed to set language");
    let tree = parser.parse(sample, None).expect("Failed to parse");
    let content = Rope::from_str(sample);

    let data = BeancountData::new(&tree, &content);
    let wrapped_data = Arc::new(data);

    // Simulate a HashMap like in the real server
    let mut data_map: HashMap<PathBuf, Arc<BeancountData>> = HashMap::new();
    data_map.insert(PathBuf::from("/test.beancount"), wrapped_data.clone());

    // Benchmark: Getting accounts (Arc::clone overhead)
    c.bench_function("arc_clone_accounts", |b| {
        b.iter(|| {
            let accounts = black_box(&wrapped_data).get_accounts();
            black_box(accounts)
        })
    });

    // Benchmark: Simulated completion - collecting all accounts
    c.bench_function("completion_collect_accounts", |b| {
        b.iter(|| {
            let mut all_accounts: Vec<String> = Vec::new();
            for bean_data in data_map.values() {
                all_accounts.extend(bean_data.get_accounts().iter().cloned());
            }
            black_box(all_accounts)
        })
    });

    // Benchmark: Multiple getter calls (tests Arc efficiency)
    c.bench_function("multiple_getter_calls", |b| {
        b.iter(|| {
            let accounts = black_box(&wrapped_data).get_accounts();
            let payees = black_box(&wrapped_data).get_payees();
            let tags = black_box(&wrapped_data).get_tags();
            let links = black_box(&wrapped_data).get_links();
            let commodities = black_box(&wrapped_data).get_commodities();
            black_box((accounts, payees, tags, links, commodities))
        })
    });

    // Benchmark: Simulated full completion scenario
    c.bench_function("full_completion_scenario", |b| {
        b.iter(|| {
            // Simulate what happens during completion
            let mut all_accounts: Vec<String> = Vec::new();
            let mut all_tags: Vec<String> = Vec::new();
            let mut all_commodities: Vec<String> = Vec::new();

            for bean_data in data_map.values() {
                all_accounts.extend(bean_data.get_accounts().iter().cloned());
                all_tags.extend(bean_data.get_tags().iter().cloned());
                all_commodities.extend(bean_data.get_commodities().iter().cloned());
            }

            all_accounts.sort();
            all_accounts.dedup();
            all_tags.sort();
            all_tags.dedup();
            all_commodities.sort();
            all_commodities.dedup();

            black_box((all_accounts, all_tags, all_commodities))
        })
    });
}

criterion_group!(benches, benchmark_data_access_patterns);
criterion_main!(benches);
