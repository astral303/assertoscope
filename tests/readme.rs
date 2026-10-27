//! Each README example, run for real; the README must contain the exact
//! message the library prints.

// The README's `consume(text)` example takes ownership.
#![allow(clippy::items_after_statements, clippy::needless_pass_by_value)]

mod support;

use assertoscope::{assert, assert_eq};
use support::{customer, failure};

const README: &str = include_str!("../README.md");

/// Fails unless the README shows `message` in a text block.
fn assert_readme_shows(message: &str) {
    let block = format!("```text\n{message}\n```");
    std::assert!(
        README.contains(&block),
        "README lacks this block:\n{message}"
    );
}

#[test]
fn opening_example() {
    let price = 12;
    let quantity = 3;
    let shipping = 5;
    let expected = 40;
    assert_readme_shows(&failure(|| {
        assert!(price * quantity + shipping == expected);
    }));
}

#[test]
fn getting_started_example_passes() {
    let prices = [12, 8, 5];
    let shipping = 5;
    assert!(prices.iter().sum::<i32>() + shipping == 30);
}

#[test]
fn method_chain_example() {
    let words = ["alpha", "beta"];
    assert_readme_shows(&failure(|| {
        assert!(words.iter().all(|word| word.len() > 4));
    }));
}

#[test]
fn field_and_index_chain_example() {
    let customer = customer();
    assert_readme_shows(&failure(|| {
        assert!(customer.orders[1].total > 20);
    }));
}

#[test]
fn short_circuit_example() {
    let items: Vec<u32> = Vec::new();
    assert_readme_shows(&failure(|| {
        assert!(!items.is_empty() && items[0] > 1);
    }));
}

#[test]
fn assert_eq_example() {
    let customer = customer();
    assert_readme_shows(&failure(|| {
        assert_eq!(customer.orders.len(), 3);
    }));
}

#[test]
fn matches_example() {
    let outcome: Result<u32, String> = Err("timeout".to_owned());
    assert_readme_shows(&failure(|| {
        assert!(matches!(outcome, Ok(n) if n > 1));
    }));
}

#[test]
fn multi_line_string_example() {
    let output = "listed=3\nwarnings=0\n";
    assert_readme_shows(&failure(|| {
        assert!(output.contains("listed=4\n"));
    }));
}

#[test]
fn message_example() {
    let order_id = "A-17";
    let actual_total = 41;
    let quoted_total = 40;
    assert_readme_shows(&failure(|| {
        assert!(
            actual_total == quoted_total,
            "order {order_id} should match its quote",
        );
    }));
}

#[test]
fn limitation_example_quotes_the_recorded_compile_error() {
    let recorded = include_str!("compile_fail/field_of_call_result_moved.stderr");
    let quoted: Vec<&str> = recorded.lines().take(7).collect();
    let block = quoted.join("\n");
    std::assert!(README.contains(&block), "README lacks this block:\n{block}");
    std::assert!(
        quoted.iter().any(|line| line.contains("assert!")),
        "the quoted lines include the assertion:\n{block}"
    );
}

#[test]
fn limitation_workarounds_compile_and_pass() {
    #[derive(Debug)]
    struct Report {
        body: Option<String>,
    }
    fn fetch() -> Report {
        Report {
            body: Some("text".to_owned()),
        }
    }
    assert!(fetch().body.as_deref() == Some("text"));
    let report = fetch();
    assert!(report.body.unwrap() == "text");
}

#[test]
fn evaluation_section_examples_compile() {
    let mut stack = vec![1, 2, 3];
    assert!(stack.pop() == Some(3));
    fn consume(text: String) -> usize {
        text.len()
    }
    let text = String::from("four");
    assert!(consume(text) == 4);
    let cell = std::cell::RefCell::new(vec![1]);
    assert!(cell.borrow().len() == 1);
}
