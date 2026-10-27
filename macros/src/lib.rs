//! Procedural macros behind [assertoscope](https://crates.io/crates/assertoscope).
//! Depend on `assertoscope` instead; it re-exports these macros and holds the
//! runtime they expand to.

use proc_macro::TokenStream;

mod compact;
mod expand;
mod lower;

use expand::{Comparison, Gate};

/// Asserts that a boolean expression is `true`. On failure the panic message
/// shows the expression with the value of each local, constant, field,
/// element, call and operator under it.
#[proc_macro]
pub fn assert(input: TokenStream) -> TokenStream {
    expand::assertion(input.into(), Gate::Always).into()
}

/// Asserts that two expressions are equal, with the diagram of `assert!`
/// drawn for `left == right`.
#[proc_macro]
pub fn assert_eq(input: TokenStream) -> TokenStream {
    expand::comparison(input.into(), Comparison::Equal, Gate::Always).into()
}

/// Asserts that two expressions are not equal, with the diagram of
/// `assert!` drawn for `left != right`.
#[proc_macro]
pub fn assert_ne(input: TokenStream) -> TokenStream {
    expand::comparison(input.into(), Comparison::NotEqual, Gate::Always).into()
}

/// `assert!` that only runs when debug assertions are enabled.
#[proc_macro]
pub fn debug_assert(input: TokenStream) -> TokenStream {
    expand::assertion(input.into(), Gate::DebugOnly).into()
}

/// `assert_eq!` that only runs when debug assertions are enabled.
#[proc_macro]
pub fn debug_assert_eq(input: TokenStream) -> TokenStream {
    expand::comparison(input.into(), Comparison::Equal, Gate::DebugOnly).into()
}

/// `assert_ne!` that only runs when debug assertions are enabled.
#[proc_macro]
pub fn debug_assert_ne(input: TokenStream) -> TokenStream {
    expand::comparison(input.into(), Comparison::NotEqual, Gate::DebugOnly).into()
}
