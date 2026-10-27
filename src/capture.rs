//! Formats a recorded value with `Debug` when the type has it.
//!
//! The macros call [`crate::__capture!`] on a reference to every recorded
//! value. The macro resolves `capture` by autoref specialization: a `Debug`
//! value picks the method on `Wrap<T>`, any other value picks the method on
//! `&Wrap<T>`. The choice is made at the call site, so no `Debug` bound
//! reaches the user's expression.

use std::fmt::{self, Debug, Write};

/// Longest `Debug` rendering kept for one value, in bytes.
///
/// An assertion with a call formats its values on every run, including runs
/// that pass. The limit caps that cost for a large collection or struct
/// whose `Debug` implementation stops at the first write error, as derived
/// and standard-library implementations do.
pub const MAX_CAPTURE_BYTES: usize = 16 * 1024;

/// Appended to a rendering cut at `MAX_CAPTURE_BYTES`.
pub const TRUNCATION_MARKER: &str = "…";

/// Wraps a reference to a recorded value so method resolution can pick the
/// `Debug` or fallback implementation.
pub struct Wrap<T>(pub T);

/// `capture` for values whose type implements `Debug`.
pub trait ViaDebug {
    /// Renders the value with `Debug`, cut at `MAX_CAPTURE_BYTES`.
    fn capture(&self) -> Option<String>;
}

impl<T: Debug> ViaDebug for Wrap<T> {
    fn capture(&self) -> Option<String> {
        Some(debug_string(&self.0))
    }
}

/// `capture` for values without `Debug`; the value is left out of the
/// diagram.
pub trait ViaFallback {
    /// Returns `None`.
    fn capture(&self) -> Option<String>;
}

impl<T> ViaFallback for &Wrap<T> {
    fn capture(&self) -> Option<String> {
        None
    }
}

/// Renders `value` with `Debug`, stopping at `MAX_CAPTURE_BYTES`.
pub fn debug_string<T: Debug + ?Sized>(value: &T) -> String {
    let mut output = Capped::new(MAX_CAPTURE_BYTES);
    // An `Err` either reports the cut or comes from the `Debug` impl itself;
    // both keep what was written so far.
    let _ = write!(output, "{value:?}");
    output.finish()
}

/// A string that refuses writes past its limit.
struct Capped {
    buffer: String,
    limit: usize,
    truncated: bool,
}

impl Capped {
    fn new(limit: usize) -> Self {
        Self {
            buffer: String::new(),
            limit,
            truncated: false,
        }
    }

    fn finish(mut self) -> String {
        if self.truncated {
            self.buffer.push_str(TRUNCATION_MARKER);
        }
        self.buffer
    }
}

impl Write for Capped {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        let room = self.limit - self.buffer.len();
        if text.len() <= room {
            self.buffer.push_str(text);
            return Ok(());
        }
        let cut = (0..=room)
            .rev()
            .find(|&index| text.is_char_boundary(index))
            .unwrap_or(0);
        self.buffer.push_str(&text[..cut]);
        self.truncated = true;
        Err(fmt::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_value_is_rendered() {
        assert_eq!(crate::__capture!(&vec![1, 2]), Some("[1, 2]".to_owned()));
    }

    #[test]
    fn value_without_debug_is_left_out() {
        struct Opaque;
        assert_eq!(crate::__capture!(&Opaque), None);
    }

    #[test]
    fn reference_to_debug_value_is_rendered() {
        let text = String::from("x");
        let reference = &text;
        assert_eq!(crate::__capture!(&reference), Some("\"x\"".to_owned()));
    }

    #[test]
    fn long_rendering_is_cut_with_marker() {
        let long = "a".repeat(MAX_CAPTURE_BYTES * 2);
        let rendered = debug_string(&long);
        assert_eq!(rendered.len(), MAX_CAPTURE_BYTES + TRUNCATION_MARKER.len());
        assert!(rendered.ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn cut_keeps_char_boundaries() {
        let mut output = Capped::new(4);
        let _ = output.write_str("ab€cd");
        assert_eq!(output.finish(), "ab…");
    }
}
