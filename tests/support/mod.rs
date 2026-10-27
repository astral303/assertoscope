//! Shared by the integration tests: the message of a failing assertion and
//! the fixtures the diagrams are drawn from.

// Each test crate uses a subset of these.
#![allow(dead_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};

/// The panic message of a failing assertion.
pub fn failure(assertion: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(assertion)).expect_err("the assertion should fail");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .expect("the panic carries a message")
}

pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Customer {
    pub name: String,
    pub orders: Vec<Order>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: u32,
    pub total: i64,
}

pub fn customer() -> Customer {
    Customer {
        name: "Ada".to_owned(),
        orders: vec![Order { id: 7, total: 41 }, Order { id: 8, total: 12 }],
    }
}
