//! Groovy-style power assertions for Rust.
//!
//! [`assert!`], [`assert_eq!`] and [`assert_ne!`] replace the standard
//! macros. When an assertion fails, the panic message shows the expression
//! with the value of each local, constant, field, element, call and
//! operator under it:
//!
//! ```should_panic
//! use assertoscope::assert;
//!
//! let prices = [12, 8, 5];
//! let shipping = 5;
//!
//! assert!(prices.iter().sum::<i32>() + shipping == 20);
//! ```
//!
//! ```text
//! assertion failed
//!
//!     prices.iter().sum::<i32>() + shipping == 20
//!     |      |      |            | |        |
//!     |      |      25           | 5        false
//!     |      Iter([12, 8, 5])    30
//!     [12, 8, 5]
//! ```
//!
//! Values are rendered with `Debug`; a value whose type has no `Debug` is
//! left out. The `debug_assert!` family is provided as well.

pub use assertoscope_macros::{
    assert, assert_eq, assert_ne, debug_assert, debug_assert_eq, debug_assert_ne,
};

mod capture;
mod recorder;
mod render;

/// Runtime support for the generated code. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::capture::{ViaDebug, ViaFallback, Wrap};
    pub use crate::recorder::{Layout, Recorder, Slot, fail};
}

/// Renders a value with `Debug` when its type has it. Not part of the public
/// API.
///
/// Takes a reference. Expands to `Some(rendering)` for a `Debug` type and to
/// `None` otherwise; see `capture.rs` for the method resolution it relies on.
#[doc(hidden)]
#[macro_export]
macro_rules! __capture {
    ($reference:expr) => {{
        #[allow(unused_imports)]
        use $crate::__private::{ViaDebug as _, ViaFallback as _};
        (&$crate::__private::Wrap($reference)).capture()
    }};
}
