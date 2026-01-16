use beancount_language_server::checkers::{
    BeancountCheckConfig, BeancountCheckMethod, create_checker,
};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Sample beancount file with realistic complexity for benchmarking
const SAMPLE_BEANCOUNT: &str = r#"
; Sample beancount file for benchmarking checker performance
option "title" "Benchmark Test File"
option "operating_currency" "USD"

; Account openings
2024-01-01 open Assets:Checking USD
2024-01-01 open Assets:Savings USD
2024-01-01 open Assets:Investment:Stocks USD
2024-01-01 open Liabilities:CreditCard USD
2024-01-01 open Expenses:Food
2024-01-01 open Expenses:Transport
2024-01-01 open Expenses:Housing:Rent
2024-01-01 open Expenses:Utilities:Electric
2024-01-01 open Expenses:Utilities:Internet
2024-01-01 open Income:Salary
2024-01-01 open Equity:Opening

; Commodity declarations
2024-01-02 commodity USD
2024-01-02 commodity EUR
2024-01-02 commodity AAPL

; Opening balance
2024-01-01 * "Opening balance"
    Assets:Checking  5000.00 USD
    Equity:Opening  -5000.00 USD

; Regular transactions
2024-01-05 * "Grocery Store" "Weekly groceries" #food #shopping
    Expenses:Food  150.00 USD
    Assets:Checking  -150.00 USD

2024-01-06 * "Gas Station" "Fuel" #transport
    Expenses:Transport  45.00 USD
    Assets:Checking  -45.00 USD

2024-01-07 * "Coffee Shop" "Morning coffee" #food
    Expenses:Food  5.50 USD
    Assets:Checking  -5.50 USD

2024-01-10 * "Rent Payment" "Monthly rent" #housing
    Expenses:Housing:Rent  1500.00 USD
    Assets:Checking  -1500.00 USD

2024-01-12 * "Electric Bill" "Monthly utility" #utilities
    Expenses:Utilities:Electric  85.00 USD
    Assets:Checking  -85.00 USD

2024-01-12 * "Internet Bill" "Monthly utility" #utilities
    Expenses:Utilities:Internet  60.00 USD
    Assets:Checking  -60.00 USD

2024-01-15 * "Employer" "Monthly salary" #income
    Income:Salary  -4500.00 USD
    Assets:Checking  4500.00 USD

2024-01-18 * "Restaurant" "Dinner with friends" #food
    Expenses:Food  85.00 USD
    Liabilities:CreditCard  -85.00 USD

2024-01-20 * "Supermarket" "Groceries" #food
    Expenses:Food  120.00 USD
    Assets:Checking  -120.00 USD

2024-01-22 * "Bus Pass" "Monthly transit pass" #transport
    Expenses:Transport  100.00 USD
    Assets:Checking  -100.00 USD

2024-01-25 * "Transfer to Savings" "Monthly savings" #savings
    Assets:Savings  500.00 USD
    Assets:Checking  -500.00 USD

2024-01-26 * "Stock Purchase" "Investment" #investment
    Assets:Investment:Stocks  10 AAPL @ 180.00 USD
    Assets:Checking  -1800.00 USD

2024-01-28 * "Pharmacy" "Medication" #health
    Expenses:Food  25.00 USD
    Assets:Checking  -25.00 USD

2024-01-30 * "Credit Card Payment" "Monthly payment" #payment
    Liabilities:CreditCard  85.00 USD
    Assets:Checking  -85.00 USD

; Balance assertions
2024-01-31 balance Assets:Checking  5484.00 USD
2024-01-31 balance Assets:Savings  500.00 USD
2024-01-31 balance Liabilities:CreditCard  0.00 USD
"#;

/// Helper to create a temporary beancount file with the given content
fn create_test_file(content: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let file_path = temp_dir.path().join("test.beancount");
    fs::write(&file_path, content).expect("Failed to write test file");
    (temp_dir, file_path)
}

/// Benchmark all three checkers if available
fn benchmark_checker_comparison(c: &mut Criterion) {
    let (_temp_dir, file_path) = create_test_file(SAMPLE_BEANCOUNT);

    let mut group = c.benchmark_group("checker_comparison");

    // SystemCall checker benchmark
    let config = BeancountCheckConfig {
        method: Some(BeancountCheckMethod::SystemCall),
        bean_check_cmd: None,
        python_cmd: None,
    };

    if let Some(checker) = create_checker(&config, Path::new(".")) {
        if checker.is_available() {
            group.bench_with_input(
                BenchmarkId::new("SystemCall", "standard"),
                &file_path,
                |b, path| {
                    b.iter(|| {
                        let _ = checker.check(black_box(path));
                    });
                },
            );
        } else {
            eprintln!("SystemCall checker not available - skipping benchmark");
        }
    }

    // SystemPython checker benchmark
    let config = BeancountCheckConfig {
        method: Some(BeancountCheckMethod::PythonSystem),
        bean_check_cmd: None,
        python_cmd: None,
    };

    if let Some(checker) = create_checker(&config, Path::new(".")) {
        if checker.is_available() {
            group.bench_with_input(
                BenchmarkId::new("SystemPython", "standard"),
                &file_path,
                |b, path| {
                    b.iter(|| {
                        let _ = checker.check(black_box(path));
                    });
                },
            );
        } else {
            eprintln!("SystemPython checker not available - skipping benchmark");
        }
    }

    // PyO3Embedded checker benchmark (requires python-embedded feature)
    #[cfg(feature = "python-embedded")]
    {
        let config = BeancountCheckConfig {
            method: Some(BeancountCheckMethod::PythonEmbedded),
            bean_check_cmd: None,
            python_cmd: None,
        };

        if let Some(checker) = create_checker(&config, Path::new(".")) {
            if checker.is_available() {
                group.bench_with_input(
                    BenchmarkId::new("PyO3Embedded", "standard"),
                    &file_path,
                    |b, path| {
                        b.iter(|| {
                            let _ = checker.check(black_box(path));
                        });
                    },
                );
            } else {
                eprintln!("PyO3Embedded checker not available - skipping benchmark");
            }
        }
    }

    #[cfg(not(feature = "python-embedded"))]
    eprintln!("PyO3Embedded checker requires python-embedded feature - skipping benchmark");

    group.finish();
}

/// Benchmark SystemCall checker with different file sizes
fn benchmark_system_call_scaling(c: &mut Criterion) {
    let config = BeancountCheckConfig {
        method: Some(BeancountCheckMethod::SystemCall),
        bean_check_cmd: None,
        python_cmd: None,
    };

    let checker = match create_checker(&config, Path::new(".")) {
        Some(c) if c.is_available() => c,
        _ => {
            eprintln!("SystemCall checker not available - skipping scaling benchmarks");
            return;
        }
    };

    let mut group = c.benchmark_group("system_call_scaling");

    // Small file
    let small_content = SAMPLE_BEANCOUNT;
    let (_temp_dir_small, file_path_small) = create_test_file(small_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "small_30_lines"),
        &file_path_small,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    // Medium file (3x repeat)
    let medium_content = SAMPLE_BEANCOUNT.repeat(3);
    let (_temp_dir_medium, file_path_medium) = create_test_file(&medium_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "medium_90_lines"),
        &file_path_medium,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    // Large file (10x repeat)
    let large_content = SAMPLE_BEANCOUNT.repeat(10);
    let (_temp_dir_large, file_path_large) = create_test_file(&large_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "large_300_lines"),
        &file_path_large,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    group.finish();
}

/// Benchmark PyO3Embedded checker with different file sizes
#[cfg(feature = "python-embedded")]
fn benchmark_pyo3_scaling(c: &mut Criterion) {
    let config = BeancountCheckConfig {
        method: Some(BeancountCheckMethod::PythonEmbedded),
        bean_check_cmd: None,
        python_cmd: None,
    };

    let checker = match create_checker(&config, Path::new(".")) {
        Some(c) if c.is_available() => c,
        _ => {
            eprintln!("PyO3Embedded checker not available - skipping scaling benchmarks");
            return;
        }
    };

    let mut group = c.benchmark_group("pyo3_scaling");

    // Small file
    let small_content = SAMPLE_BEANCOUNT;
    let (_temp_dir_small, file_path_small) = create_test_file(small_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "small_30_lines"),
        &file_path_small,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    // Medium file (3x repeat)
    let medium_content = SAMPLE_BEANCOUNT.repeat(3);
    let (_temp_dir_medium, file_path_medium) = create_test_file(&medium_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "medium_90_lines"),
        &file_path_medium,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    // Large file (10x repeat)
    let large_content = SAMPLE_BEANCOUNT.repeat(10);
    let (_temp_dir_large, file_path_large) = create_test_file(&large_content);

    group.bench_with_input(
        BenchmarkId::new("file_size", "large_300_lines"),
        &file_path_large,
        |b, path| {
            b.iter(|| {
                let _ = checker.check(black_box(path));
            });
        },
    );

    group.finish();
}

#[cfg(feature = "python-embedded")]
criterion_group!(
    benches,
    benchmark_checker_comparison,
    benchmark_system_call_scaling,
    benchmark_pyo3_scaling
);

#[cfg(not(feature = "python-embedded"))]
criterion_group!(
    benches,
    benchmark_checker_comparison,
    benchmark_system_call_scaling
);

criterion_main!(benches);
