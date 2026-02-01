use beancount_language_server::beancount_data::BeancountData;
use criterion::{Criterion, criterion_group, criterion_main};
use ropey::Rope;
use std::hint::black_box;
use tree_sitter::Parser;
use tree_sitter_beancount::tree_sitter;

fn benchmark_beancount_data_extraction(c: &mut Criterion) {
    // Sample beancount data with various directives
    let sample = r#"
; Sample beancount file for benchmarking
option "title" "Benchmark File"
option "operating_currency" "USD"

2024-01-01 open Assets:Checking USD
2024-01-01 open Assets:Savings USD
2024-01-01 open Expenses:Food
2024-01-01 open Expenses:Transport
2024-01-01 open Income:Salary

2024-01-02 commodity USD
2024-01-02 commodity EUR
2024-01-02 commodity GBP

2024-01-05 * "Grocery Store" "Weekly groceries" #food #shopping ^receipt-001
    Expenses:Food  150.00 USD
    Assets:Checking  -150.00 USD

2024-01-06 * "Gas Station" "Fuel" #transport
    Expenses:Transport  45.00 USD
    Assets:Checking  -45.00 USD

2024-01-10 ! "Important Payment" "Rent payment" #rent ^lease-2024
    Expenses:Rent  1500.00 USD
    Assets:Checking  -1500.00 USD

2024-01-15 * "Employer" "Monthly salary"
    Income:Salary  -5000.00 USD
    Assets:Checking  5000.00 USD

2024-01-20 * "Restaurant" "Dinner" #food
    Expenses:Food  75.00 USD
    Assets:Checking  -75.00 USD

2024-01-25 * "Transfer" "Savings transfer"
    Assets:Savings  500.00 USD
    Assets:Checking  -500.00 USD

2024-01-28 * "Coffee Shop" "Morning coffee" #food ^receipt-002
    Expenses:Food  5.50 USD
    Assets:Checking  -5.50 USD

2024-01-30 * "Public Transit" "Monthly pass" #transport
    Expenses:Transport  100.00 USD
    Assets:Checking  -100.00 USD
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_beancount::language())
        .expect("Failed to set language");
    let tree = parser.parse(sample, None).expect("Failed to parse");
    let content = Rope::from_str(sample);

    c.bench_function("beancount_data_extraction", |b| {
        b.iter(|| BeancountData::new(black_box(&tree), black_box(&content)))
    });
}

criterion_group!(benches, benchmark_beancount_data_extraction);
criterion_main!(benches);
