//! Groovy-style power assertions for Rust: the runtime that records values
//! while an assertion evaluates and renders the failure diagram.
//!
//! Values are rendered with `Debug`; a value whose type has no `Debug` is
//! left out.

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
