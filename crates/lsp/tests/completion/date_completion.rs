use beancount_language_server::providers::completion::add_one_month;
use beancount_language_server::providers::completion::sub_one_month;
use insta::assert_yaml_snapshot;
use lsp_types::CompletionItem;

fn complete(fixure: &str, trigger_char: Option<String>) -> Vec<CompletionItem> {}

#[test]
fn handle_sub_one_month() {
    let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
    let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 5, 1).expect("valid date");
    assert_eq!(sub_one_month(input_date), expected_date)
}

#[test]
fn handle_sub_one_month_in_jan() {
    let input_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
    let expected_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
    assert_eq!(sub_one_month(input_date), expected_date)
}

#[test]
fn handle_add_one_month() {
    let input_date = chrono::NaiveDate::from_ymd_opt(2022, 6, 1).expect("valid date");
    let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 7, 1).expect("valid date");
    assert_eq!(add_one_month(input_date), expected_date)
}

#[test]
fn handle_add_one_month_in_dec() {
    let input_date = chrono::NaiveDate::from_ymd_opt(2021, 12, 1).expect("valid date");
    let expected_date = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
    assert_eq!(add_one_month(input_date), expected_date)
}

#[test]
fn handle_date_completion() {
    assert_yaml_snapshot!(complete(
        r#"
%! bar.sp
int bar;
%! include/baz.inc
#include <>
          |
          ^
%! foo.sp
int foo;"#,
        Some('<'.to_string())
    ));
}
