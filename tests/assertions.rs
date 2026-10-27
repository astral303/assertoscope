//! The macros as a user sees them: what a failure prints, and which
//! expressions compile and evaluate as they do with the standard macros.

// Helpers sit next to the test that uses them. Some take ownership because
// the assertion under test must move its argument.
#![allow(
    clippy::items_after_statements,
    clippy::needless_pass_by_value,
    clippy::similar_names
)]

mod support;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::ops::Index;
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use assertoscope::{assert, assert_eq, assert_ne, debug_assert, debug_assert_eq, debug_assert_ne};
use support::{Order, customer, failure, lines};

struct Opaque(u8);

impl Opaque {
    fn is_even(&self) -> bool {
        self.0 % 2 == 0
    }
}

impl PartialEq for Opaque {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Order {
    fn check(&self) -> String {
        failure(|| assert!(self.total > 50))
    }
}

#[test]
fn arithmetic_shows_every_operand_and_intermediate_result() {
    let price = 12;
    let quantity = 3;
    let shipping = 5;
    let expected = 40;
    let message = failure(|| assert!(price * quantity + shipping == expected));
    let expected_message = lines(&[
        "assertion failed",
        "",
        "    price * quantity + shipping == expected",
        "    |     | |        | |        |  |",
        "    12    | 3        | 5        |  40",
        "          36         41         false",
    ]);
    assert_eq!(message, expected_message);
}

#[test]
fn method_chain_shows_the_receiver_and_each_call_result() {
    let prices = [12, 8, 5];
    let shipping = 5;
    let message = failure(|| assert!(prices.iter().sum::<i32>() + shipping == 20));
    let expected = lines(&[
        "assertion failed",
        "",
        "    prices.iter().sum::<i32>() + shipping == 20",
        "    |      |      |            | |        |",
        "    |      |      25           | 5        false",
        "    |      Iter([12, 8, 5])    30",
        "    [12, 8, 5]",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn field_and_index_chains_show_each_step() {
    let customer = customer();
    let message = failure(|| assert!(customer.orders[1].total > 20));
    let expected = lines(&[
        "assertion failed",
        "",
        "    customer.orders[1].total > 20",
        "    |        |     |   |     |",
        "    #1       |     |   12    false",
        "             |     Order { id: 8, total: 12 }",
        "             [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }]",
        "",
        "    #1 customer = Customer { name: \"Ada\", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn right_side_of_a_short_circuited_operator_is_absent() {
    let items: Vec<u32> = Vec::new();
    let message = failure(|| assert!(!items.is_empty() && items[0] > 1));
    let expected = lines(&[
        "assertion failed",
        "",
        "    !items.is_empty() && items[0] > 1",
        "    ||     |          |",
        "    |[]    true       false",
        "    false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn or_evaluates_its_right_side_only_when_the_left_is_false() {
    let evaluations = Cell::new(0);
    let right = || {
        evaluations.set(evaluations.get() + 1);
        false
    };
    assert!(true || right());
    assert_eq!(evaluations.get(), 0);
    let message = failure(|| assert!(false || right()));
    assert_eq!(evaluations.get(), 1);
    let expected = lines(&[
        "assertion failed",
        "",
        "    false || right()",
        "          |  |",
        "          |  false",
        "          false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn place_changed_by_the_left_operand_is_shown_as_the_operator_saw_it() {
    #[derive(Debug)]
    struct Counter {
        count: u32,
    }
    impl Counter {
        fn increment(&mut self) -> u32 {
            self.count += 1;
            self.count
        }
    }
    let mut counter = Counter { count: 0 };
    let message = failure(|| assert!(counter.increment() != counter.count));
    let expected = lines(&[
        "assertion failed",
        "",
        "    counter.increment() != counter.count",
        "    |       |           |  |       |",
        "    |       1           |  |       1",
        "    |                   |  Counter { count: 1 }",
        "    |                   false",
        "    Counter { count: 0 }",
    ]);
    assert_eq!(message, expected);

    fn bump(value: &mut u32) -> u32 {
        *value += 1;
        *value
    }
    let mut x = 0;
    let message = failure(|| assert!(bump(&mut x) != x));
    let expected = lines(&[
        "assertion failed",
        "",
        "    bump(&mut x) != x",
        "    |         |  |  |",
        "    1         0  |  1",
        "                 false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn message_is_formatted_only_on_failure() {
    let formatted = Cell::new(0);
    let describe = || {
        formatted.set(formatted.get() + 1);
        "A-17"
    };
    assert!(1 + 1 == 2, "order {} should match", describe());
    assert_eq!(formatted.get(), 0);
    let actual_total = 41;
    let quoted_total = 40;
    let message = failure(|| {
        assert!(
            actual_total == quoted_total,
            "order {} should match its quote",
            describe(),
        );
    });
    assert_eq!(formatted.get(), 1);
    let expected = lines(&[
        "assertion failed: order A-17 should match its quote",
        "",
        "    actual_total == quoted_total",
        "    |            |  |",
        "    41           |  40",
        "                 false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn inline_format_arguments_work_in_the_message() {
    let name = "Ada";
    let message = failure(|| assert!(name.is_empty(), "{name} is not empty"));
    assert!(
        message.starts_with("assertion failed: Ada is not empty\n"),
        "the message opens with the formatted text:\n{message}"
    );
}

#[test]
fn assert_eq_shows_both_sides_as_a_comparison() {
    let customer = customer();
    let message = failure(|| assert_eq!(customer.orders.len(), 3));
    let expected = lines(&[
        "assertion failed",
        "",
        "    customer.orders.len() == 3",
        "    |        |      |     |",
        "    #1       |      2     false",
        "             [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }]",
        "",
        "    #1 customer = Customer { name: \"Ada\", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn assert_ne_shows_the_equal_values() {
    let left = "same";
    let right = String::from("same");
    let message = failure(|| assert_ne!(left, right.as_str(), "{left} came back"));
    let expected = lines(&[
        "assertion failed: same came back",
        "",
        "    left != right.as_str()",
        "    |    |  |     |",
        "    |    |  |     \"same\"",
        "    |    |  \"same\"",
        "    |    false",
        "    \"same\"",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn assert_eq_compares_through_references_like_the_standard_macro() {
    let owned = String::from("abc");
    assert_eq!(owned, "abc");
    assert_eq!(owned.as_str(), "abc");
    assert_eq!(vec![1, 2], [1, 2]);
    assert_ne!(owned, "abd");
    // `owned` was borrowed, not moved.
    assert_eq!(owned.len(), 3);
}

#[test]
fn assert_eq_accepts_values_without_debug() {
    let left = Opaque(2);
    let right = Opaque(2);
    assert_eq!(left, right);
    let message = failure(|| assert_ne!(left, right));
    assert_eq!(
        message,
        lines(&[
            "assertion failed",
            "",
            "    left != right",
            "         |",
            "         false"
        ])
    );
}

#[test]
fn values_without_debug_are_left_out() {
    let opaque = Opaque(3);
    let message = failure(|| assert!(opaque.is_even()));
    let expected = lines(&[
        "assertion failed",
        "",
        "    opaque.is_even()",
        "           |",
        "           false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn generic_values_are_shown_only_with_a_debug_bound() {
    fn check<T: PartialEq>(value: T, other: T) -> String {
        failure(|| assert!(value == other))
    }
    fn check_debug<T: PartialEq + fmt::Debug>(value: T, other: T) -> String {
        failure(|| assert!(value == other))
    }
    assert_eq!(
        check(1, 2),
        lines(&[
            "assertion failed",
            "",
            "    value == other",
            "          |",
            "          false"
        ])
    );
    assert_eq!(
        check_debug(1, 2),
        lines(&[
            "assertion failed",
            "",
            "    value == other",
            "    |     |  |",
            "    1     |  2",
            "          false",
        ])
    );
}

#[test]
fn multi_line_string_is_listed_as_text() {
    let output = "listed=3\nwarnings=0\n";
    let message = failure(|| assert!(output.contains("listed=4\n")));
    let expected = lines(&[
        "assertion failed",
        "",
        "    output.contains(\"listed=4\\n\")",
        "    |      |",
        "    #1     false",
        "",
        "    #1 output =",
        "        listed=3",
        "        warnings=0",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn carriage_returns_and_tabs_stay_visible_in_the_legend() {
    let text = "a\r\n\tb\r\n";
    let message = failure(|| assert!(text.is_empty()));
    let expected = lines(&[
        "assertion failed",
        "",
        "    text.is_empty()",
        "    |    |",
        "    #1   false",
        "",
        "    #1 text =",
        "        a\\r",
        "        \\tb\\r",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn wide_values_are_numbered_from_the_left() {
    let names: Vec<String> = (0..12)
        .map(|index| format!("participant-{index}"))
        .collect();
    let message = failure(|| assert!(names.contains(&"participant-12".to_owned())));
    let expected = lines(&[
        "assertion failed",
        "",
        "    names.contains(&\"participant-12\".to_owned())",
        "    |     |                          |",
        "    #1    false                      \"participant-12\"",
        "",
        "    #1 names = [\"participant-0\", \"participant-1\", \"participant-2\", \"participant-3\", \"participant-4\", \"participant-5\", \"participant-6\", \"participant-7\", \"participant-8\", \"participant-9\", \"participant-10\", \"participant-11\"]",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn huge_value_is_cut() {
    let huge = "x".repeat(100_000);
    let message = failure(|| assert!(huge.is_empty()));
    assert!(
        message.len() < 20_000,
        "the message is cut near 16 KiB: {}",
        message.len()
    );
    assert!(message.contains('…'), "the cut is marked:\n{message}");
}

#[test]
fn each_call_runs_once() {
    let calls = Cell::new(0);
    let next = || {
        calls.set(calls.get() + 1);
        calls.get()
    };
    assert!(next() == 1 && next() == 2);
    assert_eq!(calls.get(), 2);
    let message = failure(|| assert!(next() + next() == 0));
    assert_eq!(calls.get(), 4);
    let expected = lines(&[
        "assertion failed",
        "",
        "    next() + next() == 0",
        "    |      | |      |",
        "    3      7 4      false",
    ]);
    assert_eq!(message, expected);
}

static DEBUG_CALLS: AtomicUsize = AtomicUsize::new(0);

struct Counted(u32);

impl fmt::Debug for Counted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        DEBUG_CALLS.fetch_add(1, Ordering::SeqCst);
        write!(f, "Counted({})", self.0)
    }
}

#[test]
// A call-free condition is linted as the user's own code, as with std.
#[allow(clippy::cast_possible_truncation)]
fn call_free_assertion_renders_values_only_on_failure() {
    let items = vec![Counted(1), Counted(2)];
    let before = DEBUG_CALLS.load(Ordering::SeqCst);
    for index in 0..items.len() {
        assert!(items[index].0 < 10);
        assert_eq!(items[index].0, index as u32 + 1);
        assert_ne!(items[index].0, 0);
    }
    assert_eq!(
        DEBUG_CALLS.load(Ordering::SeqCst),
        before,
        "a passing call-free assertion renders no value"
    );
    let message = failure(|| assert!(items[0].0 > 5));
    assert!(
        DEBUG_CALLS.load(Ordering::SeqCst) > before,
        "a failing assertion renders its values"
    );
    assert!(
        message.contains("Counted(1)"),
        "the element is shown:\n{message}"
    );
}

#[test]
fn index_runs_once_per_evaluation() {
    struct Table {
        values: [u32; 2],
        reads: Cell<usize>,
    }
    impl Index<usize> for Table {
        type Output = u32;
        fn index(&self, position: usize) -> &u32 {
            self.reads.set(self.reads.get() + 1);
            &self.values[position]
        }
    }
    let table = Table {
        values: [5, 6],
        reads: Cell::new(0),
    };
    assert!(table[1].pow(1) == 6);
    assert_eq!(
        table.reads.get(),
        1,
        "an assertion with a call indexes once"
    );
    assert!(table[1] == 6);
    assert_eq!(
        table.reads.get(),
        2,
        "a passing call-free assertion indexes once"
    );
    failure(|| assert!(table[1] == 7));
    assert_eq!(
        table.reads.get(),
        4,
        "a failing call-free assertion indexes again to collect its values"
    );
}

#[test]
fn computed_array_length_compiles() {
    let buffer = [0u8; 4];
    assert_eq!(buffer, [0u8; 2 * 2]);
    assert!([0u8; 2 + 2].len() == buffer.len());
}

#[test]
fn user_macro_expanding_to_a_field_doesnt_move_it() {
    macro_rules! name_of {
        ($customer:expr) => {
            $customer.name
        };
    }
    let user = customer();
    assert!(name_of!(user) == "Ada");
    assert_eq!(user.name, "Ada");
    let message = failure(|| assert!(name_of!(user) == "Bob"));
    let expected = lines(&[
        "assertion failed",
        "",
        "    name_of!(user) == \"Bob\"",
        "                   |",
        "                   false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn tuple_holding_a_string_is_built_once() {
    let name = String::from("abc");
    assert!((name, 1).0.len() == 3);
}

#[test]
fn moved_arguments_are_recorded_before_the_move() {
    fn consume(text: String) -> usize {
        text.len()
    }
    let text = String::from("moved");
    let message = failure(|| assert!(consume(text) == 4));
    let expected = lines(&[
        "assertion failed",
        "",
        "    consume(text) == 4",
        "    |       |     |",
        "    5       |     false",
        "            \"moved\"",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn mutable_receivers_and_borrows_compile_and_show_the_value_before_the_call() {
    let mut stack = vec![1, 2, 3];
    assert!(stack.pop() == Some(3));
    assert_eq!(stack.pop(), Some(2));
    fn push(target: &mut Vec<i32>, value: i32) -> usize {
        target.push(value);
        target.len()
    }
    assert!(push(&mut stack, 9) == 2);
    assert!(stack.len() == 2 && stack.iter().any(|&value| value == 9));
    let message = failure(|| assert!(stack.pop() == Some(8)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    stack.pop() == Some(8)",
        "    |     |     |",
        "    |     |     false",
        "    |     Some(9)",
        "    [1, 9]",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn temporaries_live_until_the_condition_is_evaluated() {
    fn make() -> String {
        String::from("fresh")
    }
    let cell = RefCell::new(vec![1, 2]);
    assert!(make().as_str() == "fresh");
    assert!(cell.borrow().as_slice() == [1, 2]);
    assert_eq!(make().trim(), "fresh");
    let message = failure(|| assert!(cell.borrow().first() == Some(&3)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    cell.borrow().first() == Some(&3)",
        "    |    |        |       |",
        "    |    [1, 2]   Some(1) false",
        "    RefCell { value: [1, 2] }",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn fields_reached_through_a_call_are_recorded_by_reference() {
    let mut registry: HashMap<&str, support::Customer> = HashMap::new();
    registry.insert("ada", customer());
    let message = failure(|| assert!(registry.get("ada").unwrap().orders[0].total == 40));
    let expected = lines(&[
        "assertion failed",
        "",
        "    registry.get(\"ada\").unwrap().orders[0].total == 40",
        "    |        |          |        |     |   |     |",
        "    #1       #2         #3       |     |   41    false",
        "                                 |     Order { id: 7, total: 41 }",
        "                                 [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }]",
        "",
        "    #1 registry = {\"ada\": Customer { name: \"Ada\", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }}",
        "",
        "    #2 registry.get(\"ada\") = Some(Customer { name: \"Ada\", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] })",
        "",
        "    #3 registry.get(\"ada\").unwrap() = Customer { name: \"Ada\", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn matches_shows_the_scrutinee() {
    let outcome: Result<u32, String> = Err("timeout".to_owned());
    let message = failure(|| assert!(matches!(outcome, Ok(n) if n > 1)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    matches!(outcome, Ok(n) if n > 1)",
        "    |        |",
        "    false    Err(\"timeout\")",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn closures_are_printed_without_being_recorded() {
    let words = ["alpha", "beta"];
    let message = failure(|| assert!(words.iter().all(|word| word.len() > 4)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    words.iter().all(|word| word.len() > 4)",
        "    |     |      |",
        "    |     |      false",
        "    |     Iter([\"alpha\", \"beta\"])",
        "    [\"alpha\", \"beta\"]",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn constants_are_shown_and_unit_variants_are_not() {
    const LIMIT: usize = 3;
    let found: Option<usize> = None;
    let message = failure(|| assert!(found == Some(LIMIT)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    found == Some(LIMIT)",
        "    |     |       |",
        "    None  false   3",
    ]);
    assert_eq!(message, expected);
}

#[test]
// The operators under test are the point, not the shortest expression.
#[allow(clippy::nonminimal_bool, clippy::cast_possible_wrap)]
fn unary_operators_casts_and_try_are_shown_at_their_operator() {
    let done = false;
    let message = failure(|| assert!(!done == done));
    assert_eq!(
        message,
        lines(&[
            "assertion failed",
            "",
            "    !done == done",
            "    ||    |  |",
            "    ||    |  false",
            "    ||    false",
            "    |false",
            "    true",
        ])
    );
    let count: u8 = 200;
    let message = failure(|| assert!(count as i8 > 0));
    assert_eq!(
        message,
        lines(&[
            "assertion failed",
            "",
            "    count as i8 > 0",
            "    |     |     |",
            "    200   -56   false",
        ])
    );
    fn parse(text: &str) -> Result<(), std::num::ParseIntError> {
        assert!(text.parse::<i32>()? > 10);
        Ok(())
    }
    let message = failure(|| parse("7").expect("parses"));
    assert_eq!(
        message,
        lines(&[
            "assertion failed",
            "",
            "    text.parse::<i32>()? > 10",
            "    |    |             | |",
            "    \"7\"  Ok(7)         7 false",
        ])
    );
    assert!(parse("x").is_err());
}

#[test]
fn struct_literals_and_tuples_show_their_parts() {
    let x = 1;
    let y = 2;
    let message = failure(|| assert_eq!((x, y), (1, 3)));
    let expected = lines(&[
        "assertion failed",
        "",
        "    (x, y) == (1, 3)",
        "     |  |  |",
        "     1  2  false",
    ]);
    assert_eq!(message, expected);
    let message = failure(|| assert!(Order { id: x, total: 0 } == Order { id: 2, total: 0 }));
    let expected = lines(&[
        "assertion failed",
        "",
        "    Order { id: x, total: 0 } == Order { id: 2, total: 0 }",
        "                |             |",
        "                1             false",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn self_is_shown_like_any_other_local() {
    let message = Order { id: 1, total: 41 }.check();
    let expected = lines(&[
        "assertion failed",
        "",
        "    self.total > 50",
        "    |    |     |",
        "    |    41    false",
        "    Order { id: 1, total: 41 }",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn multi_line_assertion_prints_on_one_line() {
    let customer = customer();
    let message = failure(|| {
        assert!(customer.orders.iter().map(|order| order.total).sum::<i64>() > 100);
    });
    assert!(
        message.contains("    customer.orders.iter().map(|order| order.total).sum::<i64>() > 100"),
        "the source is printed on one line:\n{message}"
    );
}

#[test]
fn string_literal_written_across_lines_prints_on_one_line() {
    let text = "first";
    let message = failure(|| {
        assert!(
            text == "first
second"
        );
    });
    let expected = lines(&[
        "assertion failed",
        "",
        "    text == \"first\\nsecond\"",
        "    |    |",
        "    |    false",
        "    \"first\"",
    ]);
    assert_eq!(message, expected);
}

#[test]
fn trailing_commas_are_accepted() {
    assert!(true,);
    assert!(true, "unused",);
    assert_eq!(1, 1,);
    assert_eq!(1, 1, "unused",);
    assert_ne!(1, 2,);
}

#[test]
fn passing_assertions_return_unit() {
    let value = 3;
    let result: () = assert!(value > 2);
    let result_eq: () = assert_eq!(value, 3);
    let result_ne: () = assert_ne!(value, 4);
    assert_eq!((result, result_eq, result_ne), ((), (), ()));
}

#[test]
fn debug_assertions_follow_the_build_profile() {
    let value = 1;
    debug_assert!(value == 1);
    debug_assert_eq!(value, 1);
    debug_assert_ne!(value, 2);
    let outcome = catch_unwind(|| debug_assert!(value == 2));
    assert_eq!(outcome.is_err(), cfg!(debug_assertions));
    let outcome = catch_unwind(|| debug_assert_eq!(value, 2));
    assert_eq!(outcome.is_err(), cfg!(debug_assertions));
    let outcome = catch_unwind(|| debug_assert_ne!(value, 1));
    assert_eq!(outcome.is_err(), cfg!(debug_assertions));
}

#[test]
fn panics_at_the_assertion_line() {
    let recorded: Arc<Mutex<Option<(String, u32)>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&recorded);
    let this_thread = std::thread::current().id();
    // Other tests panic on their own threads meanwhile; they keep the
    // previous hook.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == this_thread {
            let location = info
                .location()
                .map(|location| (location.file().to_owned(), location.line()));
            *sink.lock().expect("the hook holds the lock alone") = location;
        } else {
            previous(info);
        }
    }));
    let line = line!() + 1;
    let outcome = catch_unwind(|| assert!(1 > 2));
    let _ = std::panic::take_hook();
    assert!(outcome.is_err());
    let location = recorded
        .lock()
        .expect("the test holds the lock alone")
        .clone();
    assert_eq!(location, Some((file!().to_owned(), line)));
}

#[test]
fn works_through_a_macro_rules_wrapper() {
    macro_rules! check {
        ($condition:expr) => {
            assert!($condition)
        };
    }
    let total = 3;
    check!(total == 3);
    let message = failure(|| check!(total == 4));
    let expected = lines(&[
        "assertion failed",
        "",
        "    total == 4",
        "    |     |",
        "    3     false",
    ]);
    assert_eq!(message, expected);
}
