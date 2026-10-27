//! Parses each macro's input and assembles the generated block.
//!
//! Every macro expands to a block that evaluates the condition into a
//! `bool` and, on `false`, panics with the diagram. A call-free condition
//! runs as written first and is evaluated again with records only when it
//! fails, so a passing assertion costs what the standard macro costs. A
//! condition with a call runs once, with records, because a second run
//! could repeat the call's effects.

use proc_macro2::{Ident, TokenStream};
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Token};

use crate::lower::{Lowering, Mode, Slot, hygienic, is_call_free, located};

struct AssertionInput {
    condition: Expr,
    message: Option<TokenStream>,
}

impl Parse for AssertionInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let condition = input.parse()?;
        let message = parse_message(input)?;
        Ok(Self { condition, message })
    }
}

struct ComparisonInput {
    left: Expr,
    right: Expr,
    message: Option<TokenStream>,
}

impl Parse for ComparisonInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let left = input.parse()?;
        input.parse::<Token![,]>()?;
        let right = input.parse()?;
        let message = parse_message(input)?;
        Ok(Self {
            left,
            right,
            message,
        })
    }
}

/// The optional `, format, args...` after the expressions.
fn parse_message(input: ParseStream<'_>) -> syn::Result<Option<TokenStream>> {
    if input.is_empty() {
        return Ok(None);
    }
    input.parse::<Token![,]>()?;
    let rest: TokenStream = input.parse()?;
    Ok((!rest.is_empty()).then_some(rest))
}

#[derive(Clone, Copy)]
pub(crate) enum Comparison {
    Equal,
    NotEqual,
}

/// When the assertion runs.
#[derive(Clone, Copy)]
pub(crate) enum Gate {
    Always,
    DebugOnly,
}

pub(crate) fn assertion(input: TokenStream, gate: Gate) -> TokenStream {
    let arguments = match syn::parse2::<AssertionInput>(input) {
        Ok(arguments) => arguments,
        Err(error) => return error.to_compile_error(),
    };
    let names = Names::new();
    let mut lowering = Lowering::new(names.recorder.clone());
    let lowered = lowering.lower(&arguments.condition, Mode::Record);
    let instrumented = lowering.after_records(&lowered.pending, lowered.tokens);
    let (source, slots) = lowering.finish();
    let block = Block {
        names: &names,
        source: &source,
        slots: &slots,
        message: arguments.message,
    };
    // A condition that is not `bool` is reported at the condition.
    let span = located(arguments.condition.span());
    let passed = &names.passed;
    let generated = if is_call_free(&arguments.condition) {
        let condition = &arguments.condition;
        block.two_pass(
            &quote_spanned!(span=> let #passed: bool = #condition;),
            &quote_spanned!(span=> let _: bool = #instrumented;),
        )
    } else {
        block.single_pass(&quote_spanned!(span=> let #passed: bool = #instrumented;))
    };
    gated(generated, gate)
}

pub(crate) fn comparison(input: TokenStream, comparison: Comparison, gate: Gate) -> TokenStream {
    let arguments = match syn::parse2::<ComparisonInput>(input) {
        Ok(arguments) => arguments,
        Err(error) => return error.to_compile_error(),
    };
    let names = Names::new();
    let mut lowering = Lowering::new(names.recorder.clone());
    let left = lowering.lower(&arguments.left, Mode::Record);
    lowering.push(" ");
    let column = lowering.column();
    lowering.push(match comparison {
        Comparison::Equal => "==",
        Comparison::NotEqual => "!=",
    });
    lowering.push(" ");
    let right = lowering.lower(&arguments.right, Mode::Record);
    let slot = lowering.add_slot_for_whole_text(column);

    let (left_records, right_records) = (
        lowering.record_calls(&left.pending),
        lowering.record_calls(&right.pending),
    );
    let (left_skips, right_skips) = (
        left_records.iter().map(|_| quote!(_)),
        right_records.iter().map(|_| quote!(_)),
    );
    let (left_value, right_value) = (hygienic("__left"), hygienic("__right"));
    let (left_tokens, right_tokens) = (left.tokens, right.tokens);
    let compare = match comparison {
        Comparison::Equal => quote!(*#left_value == *#right_value),
        Comparison::NotEqual => quote!(*#left_value != *#right_value),
    };
    let outcome = hygienic("__outcome");
    let record = lowering.record_call(slot, &quote!(&#outcome));
    let passed = &names.passed;
    // One `match` keeps every temporary alive until both sides are compared,
    // as the standard `assert_eq!` does.
    let instrumented = quote! {
        let #passed: bool = match (
            #(#left_records,)* &(#left_tokens),
            #(#right_records,)* &(#right_tokens),
        ) {
            (#(#left_skips,)* #left_value, #(#right_skips,)* #right_value) => {
                let #outcome = #compare;
                #record;
                #outcome
            }
        };
    };
    let (source, slots) = lowering.finish();
    let block = Block {
        names: &names,
        source: &source,
        slots: &slots,
        message: arguments.message,
    };
    let generated = if is_call_free(&arguments.left) && is_call_free(&arguments.right) {
        let (left, right) = (&arguments.left, &arguments.right);
        let plain = quote! {
            let #passed: bool = match (&(#left), &(#right)) {
                (#left_value, #right_value) => #compare,
            };
        };
        block.two_pass(&plain, &instrumented)
    } else {
        block.single_pass(&instrumented)
    };
    gated(generated, gate)
}

/// The hygienic locals every generated block declares.
struct Names {
    recorder: Ident,
    passed: Ident,
}

impl Names {
    fn new() -> Self {
        Self {
            recorder: hygienic("__recorder"),
            passed: hygienic("__passed"),
        }
    }
}

struct Block<'a> {
    names: &'a Names,
    source: &'a str,
    slots: &'a [Slot],
    message: Option<TokenStream>,
}

impl Block<'_> {
    /// `evaluate` declares `passed` while recording; the recorder exists
    /// throughout.
    fn single_pass(&self, evaluate: &TokenStream) -> TokenStream {
        let recorder = self.declare_recorder();
        let failure = self.failure();
        let passed = &self.names.passed;
        quote! {{
            #recorder
            #evaluate
            if !#passed { #failure }
        }}
    }

    /// `plain` declares `passed` without records; `evaluate_again` runs the
    /// recording version only after a failure.
    fn two_pass(&self, plain: &TokenStream, evaluate_again: &TokenStream) -> TokenStream {
        let recorder = self.declare_recorder();
        let failure = self.failure();
        let passed = &self.names.passed;
        quote! {{
            #plain
            if !#passed {
                #recorder
                #evaluate_again
                #failure
            }
        }}
    }

    fn declare_recorder(&self) -> TokenStream {
        let recorder = &self.names.recorder;
        let slot_count = self.slots.len();
        let mutability = (slot_count > 0).then(|| quote!(mut));
        quote!(let #mutability #recorder = ::assertoscope::__private::Recorder::new(#slot_count);)
    }

    fn failure(&self) -> TokenStream {
        let recorder = &self.names.recorder;
        let source = self.source;
        let slots = self.slots.iter().map(|slot| {
            let (column, start, end) = (slot.column, slot.start, slot.end);
            quote!(::assertoscope::__private::Slot { column: #column, start: #start, end: #end })
        });
        let message = if let Some(message) = &self.message {
            quote!(::core::option::Option::Some(::std::format!(#message)))
        } else {
            quote!(::core::option::Option::None)
        };
        quote! {
            ::assertoscope::__private::fail(
                &::assertoscope::__private::Layout {
                    source: #source,
                    slots: &[#(#slots),*],
                },
                &#recorder,
                #message,
            )
        }
    }
}

fn gated(block: TokenStream, gate: Gate) -> TokenStream {
    match gate {
        Gate::Always => block,
        Gate::DebugOnly => quote!(if ::core::cfg!(debug_assertions) #block),
    }
}
