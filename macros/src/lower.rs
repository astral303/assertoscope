//! Walks an assertion expression once: prints it on one line, and rewrites
//! it so each recorded subexpression stores its value as it is evaluated.
//!
//! A value expression (a call, an operator, a cast, ...) becomes
//! `match (expr) { v => { record(&v); v } }`, which stores the value and
//! yields it, moving it on to its consumer.
//!
//! A local, a constant, or a field of one is left in place and recorded by
//! reference just before the nearest enclosing value expression evaluates.
//! The expression can still move it, borrow it mutably or call a `&mut
//! self` method on it. A `#[repr(packed)]` field and a `static mut` cannot
//! be referenced, so an assertion that reads one does not compile.
//!
//! An element, or a field of a value expression, is recorded through a
//! reference: `*match &base[i] { r => { record(r); r } }`. The result is a
//! place that can be read, compared and borrowed; it cannot be moved out of
//! or borrowed mutably.
//!
//! Closures, blocks, `if`, `match` and macro arguments are printed but not
//! walked. `matches!` is the exception: its scrutinee is walked.

use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    BinOp, Expr, ExprBinary, ExprCall, ExprCast, ExprClosure, ExprField, ExprIndex, ExprMacro,
    ExprMethodCall, ExprPath, ExprRange, ExprRepeat, ExprStruct, ExprUnary, Macro, Member, Pat,
    ReturnType, Token, UnOp,
};

use crate::compact::compact;

/// Position of a recorded subexpression in the printed text.
pub(crate) struct Slot {
    /// Character offset of the token the value is shown under.
    pub column: usize,
    /// Byte offset where the subexpression's text starts.
    pub start: usize,
    /// Byte offset where the subexpression's text ends.
    pub end: usize,
}

/// A place expression to record by reference, hoisted to the nearest
/// enclosing value expression.
pub(crate) struct Pending {
    slot: usize,
    place: TokenStream,
}

/// `Pure` marks an expression that can be evaluated again without a side
/// effect or a second record: a literal, a local, a constant, a field of
/// one, or a range of those. A `Deref` implementation on the way to a field
/// is the exception; it runs on each evaluation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purity {
    Pure,
    Impure,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Print and instrument.
    Record,
    /// Print only; the expression's tokens are returned unchanged.
    PrintOnly,
}

pub(crate) struct Lowered {
    pub tokens: TokenStream,
    pub pending: Vec<Pending>,
    pub purity: Purity,
}

impl Lowered {
    fn pure(tokens: TokenStream) -> Self {
        Self {
            tokens,
            pending: Vec::new(),
            purity: Purity::Pure,
        }
    }

    fn impure(tokens: TokenStream) -> Self {
        Self {
            tokens,
            pending: Vec::new(),
            purity: Purity::Impure,
        }
    }
}

struct LoweredList {
    /// Each element's tokens, parenthesized.
    tokens: Vec<TokenStream>,
    pending: Vec<Pending>,
    purity: Purity,
}

pub(crate) struct Lowering {
    text: String,
    width: usize,
    slots: Vec<Slot>,
    recorder: Ident,
}

/// An identifier the user's code cannot name or shadow.
pub(crate) fn hygienic(name: &str) -> Ident {
    Ident::new(name, Span::mixed_site())
}

/// The macro's context at the location of one of the user's tokens. A
/// compiler error on the generated expression is reported at that token,
/// while lints still treat the generated tokens as macro output.
///
/// Stable `proc_macro` cannot join the spans of a subexpression, so one
/// token is the most an error can point at. Callers pass the token the
/// diagram shows the value under where there is one, and the first token
/// of the subexpression otherwise.
pub(crate) fn located(token: Span) -> Span {
    Span::call_site().located_at(token)
}

/// True when evaluating `expr` twice reads the same places and calls
/// nothing: no call, method call, macro, closure, `?`, `.await` or block.
/// An `Index` or `Deref` implementation may run twice.
pub(crate) fn is_call_free(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Paren(paren) => is_call_free(&paren.expr),
        Expr::Group(group) => is_call_free(&group.expr),
        Expr::Field(field) => is_call_free(&field.base),
        Expr::Index(index) => is_call_free(&index.expr) && is_call_free(&index.index),
        Expr::Unary(unary) => is_call_free(&unary.expr),
        Expr::Binary(binary) => is_call_free(&binary.left) && is_call_free(&binary.right),
        Expr::Cast(cast) => is_call_free(&cast.expr),
        Expr::Reference(reference) => is_call_free(&reference.expr),
        Expr::Tuple(tuple) => tuple.elems.iter().all(is_call_free),
        Expr::Array(array) => array.elems.iter().all(is_call_free),
        Expr::Repeat(repeat) => is_call_free(&repeat.expr),
        Expr::Range(range) => {
            range.start.as_deref().is_none_or(is_call_free)
                && range.end.as_deref().is_none_or(is_call_free)
        }
        Expr::Struct(structure) => {
            structure
                .fields
                .iter()
                .all(|field| is_call_free(&field.expr))
                && structure.rest.as_deref().is_none_or(is_call_free)
        }
        _ => false,
    }
}

impl Lowering {
    pub fn new(recorder: Ident) -> Self {
        Self {
            text: String::new(),
            width: 0,
            slots: Vec::new(),
            recorder,
        }
    }

    pub fn finish(self) -> (String, Vec<Slot>) {
        (self.text, self.slots)
    }

    pub fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.width += text.chars().count();
    }

    /// Character offset where the next token will be printed.
    pub fn column(&self) -> usize {
        self.width
    }

    /// Byte offset where the next token will be printed.
    fn begin(&self) -> usize {
        self.text.len()
    }

    /// Adds a slot shown under `column` whose text is everything printed so
    /// far.
    pub fn add_slot_for_whole_text(&mut self, column: usize) -> usize {
        self.slots.push(Slot {
            column,
            start: 0,
            end: self.text.len(),
        });
        self.slots.len() - 1
    }

    fn add_slot(&mut self, mode: Mode, column: usize, start: usize) -> Option<usize> {
        if mode == Mode::PrintOnly {
            return None;
        }
        self.slots.push(Slot {
            column,
            start,
            end: self.text.len(),
        });
        Some(self.slots.len() - 1)
    }

    /// `recorder.record(slot, capture(reference))` for a reference to the
    /// recorded value.
    pub fn record_call(&self, slot: usize, reference: &TokenStream) -> TokenStream {
        let recorder = &self.recorder;
        quote!(#recorder.record(#slot, ::assertoscope::__capture!(#reference)))
    }

    pub fn record_calls(&self, pending: &[Pending]) -> Vec<TokenStream> {
        pending
            .iter()
            .map(|pending| {
                let place = &pending.place;
                self.record_call(pending.slot, &quote!(&#place))
            })
            .collect()
    }

    /// Runs the pending records, then `last`, yielding `last`'s value.
    pub fn after_records(&self, pending: &[Pending], last: TokenStream) -> TokenStream {
        if pending.is_empty() {
            return last;
        }
        let records = self.record_calls(pending);
        let index = syn::Index::from(pending.len());
        quote!(( #(#records,)* #last ).#index)
    }

    /// Wraps a value expression so its value is recorded and passed on.
    fn record_value(
        &mut self,
        mode: Mode,
        column: usize,
        start: usize,
        tokens: TokenStream,
        pending: &[Pending],
        span: Span,
    ) -> Lowered {
        let Some(slot) = self.add_slot(mode, column, start) else {
            return Lowered::impure(tokens);
        };
        let scrutinee = self.after_records(pending, tokens);
        let value = hygienic("__value");
        let record = self.record_call(slot, &quote!(&#value));
        Lowered::impure(quote_spanned!(span=> match (#scrutinee) { #value => { #record; #value } }))
    }

    /// Leaves a place expression as written and queues its record for the
    /// enclosing value expression.
    fn pend_place_record(
        &mut self,
        mode: Mode,
        column: usize,
        start: usize,
        tokens: TokenStream,
        mut pending: Vec<Pending>,
    ) -> Lowered {
        if let Some(slot) = self.add_slot(mode, column, start) {
            pending.push(Pending {
                slot,
                place: tokens.clone(),
            });
        }
        Lowered {
            tokens,
            pending,
            purity: Purity::Pure,
        }
    }

    /// Records a place through a reference and yields the place behind it.
    fn record_through_reference(
        &mut self,
        mode: Mode,
        column: usize,
        start: usize,
        tokens: TokenStream,
        pending: &[Pending],
        span: Span,
    ) -> Lowered {
        let Some(slot) = self.add_slot(mode, column, start) else {
            return Lowered::impure(tokens);
        };
        let scrutinee = self.after_records(pending, quote!(&#tokens));
        let place = hygienic("__place");
        let record = self.record_call(slot, &quote!(#place));
        Lowered::impure(quote_spanned!(span=> *match #scrutinee { #place => { #record; #place } }))
    }

    pub fn lower(&mut self, expr: &Expr, mode: Mode) -> Lowered {
        let lowered = self.lower_variant(expr, mode);
        match mode {
            Mode::Record => lowered,
            Mode::PrintOnly => Lowered::impure(expr.to_token_stream()),
        }
    }

    fn lower_variant(&mut self, expr: &Expr, mode: Mode) -> Lowered {
        match expr {
            Expr::Paren(paren) => {
                self.push("(");
                let inner = self.lower(&paren.expr, mode);
                self.push(")");
                let tokens = inner.tokens;
                Lowered {
                    tokens: quote_spanned!(located(paren.paren_token.span.join())=> (#tokens)),
                    pending: inner.pending,
                    purity: inner.purity,
                }
            }
            Expr::Group(group) => self.lower(&group.expr, mode),
            Expr::Lit(literal) => {
                self.push(&compact(literal.to_token_stream()));
                Lowered::pure(literal.to_token_stream())
            }
            Expr::Path(path) => self.lower_path(path, mode),
            Expr::Field(field) => self.lower_field(field, mode),
            Expr::Index(index) => self.lower_index(index, mode),
            Expr::MethodCall(call) => self.lower_method_call(call, mode),
            Expr::Call(call) => self.lower_call(call, mode),
            Expr::Binary(binary) => self.lower_binary(binary, mode),
            Expr::Unary(unary) => self.lower_unary(unary, mode),
            Expr::Reference(reference) => {
                let mutability = &reference.mutability;
                self.push(if mutability.is_some() { "&mut " } else { "&" });
                let inner = self.lower(&reference.expr, mode);
                let tokens = inner.tokens;
                let span = located(reference.and_token.span());
                Lowered {
                    tokens: quote_spanned!(span=> & #mutability (#tokens)),
                    pending: inner.pending,
                    purity: inner.purity,
                }
            }
            Expr::Cast(cast) => self.lower_cast(cast, mode),
            Expr::Try(try_expr) => {
                let start = self.begin();
                let inner = self.lower(&try_expr.expr, mode);
                let column = self.column();
                self.push("?");
                let tokens = inner.tokens;
                let span = located(try_expr.question_token.span());
                let tokens = quote_spanned!(span=> (#tokens)?);
                self.record_value(mode, column, start, tokens, &inner.pending, span)
            }
            Expr::Await(await_expr) => {
                let start = self.begin();
                let inner = self.lower(&await_expr.base, mode);
                self.push(".");
                let column = self.column();
                self.push("await");
                let tokens = inner.tokens;
                let span = located(await_expr.await_token.span());
                let tokens = quote_spanned!(span=> (#tokens).await);
                self.record_value(mode, column, start, tokens, &inner.pending, span)
            }
            Expr::Tuple(tuple) => {
                self.push("(");
                let elements = self.lower_list(&tuple.elems, mode);
                let trailing_comma = (tuple.elems.len() == 1).then(|| quote!(,));
                if trailing_comma.is_some() {
                    self.push(",");
                }
                self.push(")");
                let tokens = elements.tokens;
                Lowered::impure_with(quote!(( #(#tokens),* #trailing_comma )), elements.pending)
            }
            Expr::Array(array) => {
                self.push("[");
                let elements = self.lower_list(&array.elems, mode);
                self.push("]");
                let tokens = elements.tokens;
                Lowered::impure_with(quote!([ #(#tokens),* ]), elements.pending)
            }
            Expr::Repeat(repeat) => self.lower_repeat(repeat, mode),
            Expr::Range(range) => self.lower_range(range, mode),
            Expr::Struct(structure) => self.lower_struct(structure, mode),
            Expr::Macro(macro_call) => self.lower_macro(macro_call, mode),
            Expr::Closure(closure) => self.lower_closure(closure),
            Expr::Block(_) | Expr::If(_) | Expr::Match(_) | Expr::Unsafe(_) => {
                let start = self.begin();
                let column = self.column();
                self.push(&compact(expr.to_token_stream()));
                let tokens = expr.to_token_stream();
                self.record_value(mode, column, start, tokens, &[], located(expr.span()))
            }
            other => {
                self.push(&compact(other.to_token_stream()));
                Lowered::impure(other.to_token_stream())
            }
        }
    }

    fn lower_cast(&mut self, cast: &ExprCast, mode: Mode) -> Lowered {
        let start = self.begin();
        let inner = self.lower(&cast.expr, mode);
        self.push(" ");
        let column = self.column();
        self.push("as ");
        self.push(&compact(cast.ty.to_token_stream()));
        let ty = &cast.ty;
        let tokens = inner.tokens;
        let span = located(cast.as_token.span());
        let tokens = quote_spanned!(span=> (#tokens) as #ty);
        if is_literal(&cast.expr) {
            Lowered::pure(tokens)
        } else {
            self.record_value(mode, column, start, tokens, &inner.pending, span)
        }
    }

    /// `[element; length]`: the length is a constant expression, which a
    /// `match` or a record call cannot appear in, so it is printed only.
    fn lower_repeat(&mut self, repeat: &ExprRepeat, mode: Mode) -> Lowered {
        self.push("[");
        let element = self.lower(&repeat.expr, mode);
        self.push("; ");
        self.lower(&repeat.len, Mode::PrintOnly);
        self.push("]");
        let (element_tokens, length) = (element.tokens, &repeat.len);
        Lowered::impure_with(quote!([(#element_tokens); #length]), element.pending)
    }

    fn lower_list(&mut self, items: &Punctuated<Expr, Token![,]>, mode: Mode) -> LoweredList {
        let mut list = LoweredList {
            tokens: Vec::new(),
            pending: Vec::new(),
            purity: Purity::Pure,
        };
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.push(", ");
            }
            let lowered = self.lower(item, mode);
            let tokens = lowered.tokens;
            list.tokens
                .push(quote_spanned!(located(item.span())=> (#tokens)));
            list.pending.extend(lowered.pending);
            list.purity = combine(list.purity, lowered.purity);
        }
        list
    }

    /// Prints the path and returns the column of its last segment's name.
    fn print_path(&mut self, path: &ExprPath) -> usize {
        let text = compact(path.to_token_stream());
        let column = self.column() + text[..last_segment_offset(path, &text)].chars().count();
        self.push(&text);
        column
    }

    fn lower_path(&mut self, path: &ExprPath, mode: Mode) -> Lowered {
        let start = self.begin();
        let column = self.print_path(path);
        match classify(path) {
            PathKind::Local | PathKind::Constant => {
                self.pend_place_record(mode, column, start, path.to_token_stream(), Vec::new())
            }
            PathKind::Constructor | PathKind::Function => Lowered::pure(path.to_token_stream()),
        }
    }

    fn lower_field(&mut self, field: &ExprField, mode: Mode) -> Lowered {
        let start = self.begin();
        let base = self.lower(&field.base, mode);
        self.push(".");
        let column = self.column();
        self.push(&member_text(&field.member));
        let member = &field.member;
        let base_tokens = base.tokens;
        let span = located(field.member.span());
        let tokens = quote_spanned!(span=> (#base_tokens).#member);
        match base.purity {
            Purity::Pure => self.pend_place_record(mode, column, start, tokens, base.pending),
            Purity::Impure => {
                self.record_through_reference(mode, column, start, tokens, &base.pending, span)
            }
        }
    }

    /// An element is recorded through a reference, so `Index` runs once.
    fn lower_index(&mut self, index: &ExprIndex, mode: Mode) -> Lowered {
        let start = self.begin();
        let base = self.lower(&index.expr, mode);
        let column = self.column();
        self.push("[");
        let subscript = self.lower(&index.index, mode);
        self.push("]");
        let (base_tokens, subscript_tokens) = (base.tokens, subscript.tokens);
        let span = located(index.bracket_token.span.join());
        let tokens = quote_spanned!(span=> (#base_tokens)[#subscript_tokens]);
        let mut pending = base.pending;
        pending.extend(subscript.pending);
        self.record_through_reference(mode, column, start, tokens, &pending, span)
    }

    fn lower_method_call(&mut self, call: &ExprMethodCall, mode: Mode) -> Lowered {
        let start = self.begin();
        let receiver = self.lower(&call.receiver, mode);
        self.push(".");
        let column = self.column();
        self.push(&call.method.to_string());
        if let Some(turbofish) = &call.turbofish {
            self.push(&compact(turbofish.to_token_stream()));
        }
        self.push("(");
        let args = self.lower_list(&call.args, mode);
        self.push(")");
        let mut pending = receiver.pending;
        pending.extend(args.pending);
        let (receiver_tokens, method, turbofish) = (receiver.tokens, &call.method, &call.turbofish);
        let arg_tokens = args.tokens;
        let span = located(call.method.span());
        let tokens =
            quote_spanned!(span=> (#receiver_tokens).#method #turbofish (#(#arg_tokens),*));
        self.record_value(mode, column, start, tokens, &pending, span)
    }

    fn lower_call(&mut self, call: &ExprCall, mode: Mode) -> Lowered {
        let start = self.begin();
        let (recorded, column) = match &*call.func {
            Expr::Path(path) => {
                let column = self.print_path(path);
                (classify(path) != PathKind::Constructor, column)
            }
            callee => {
                self.lower(callee, Mode::PrintOnly);
                (true, self.column())
            }
        };
        self.push("(");
        let args = self.lower_list(&call.args, mode);
        self.push(")");
        let callee = &call.func;
        let arg_tokens = args.tokens;
        let span = located(call.paren_token.span.join());
        let tokens = quote_spanned!(span=> (#callee)(#(#arg_tokens),*));
        if recorded {
            self.record_value(mode, column, start, tokens, &args.pending, span)
        } else {
            Lowered {
                tokens,
                pending: args.pending,
                purity: args.purity,
            }
        }
    }

    /// The right operand's places are recorded after the left operand has
    /// run, so a value the left operand changed is shown as the operator
    /// saw it. A comparison borrows the operand; any other operator takes
    /// it by value, as the operator itself does.
    fn lower_binary(&mut self, binary: &ExprBinary, mode: Mode) -> Lowered {
        let start = self.begin();
        let left = self.lower(&binary.left, mode);
        self.push(" ");
        let column = self.column();
        self.push(&compact(binary.op.to_token_stream()));
        self.push(" ");
        let right = self.lower(&binary.right, mode);
        let operator = &binary.op;
        let (left_tokens, right_tokens) = (left.tokens, right.tokens);
        let right_tokens = if right.pending.is_empty() {
            quote!((#right_tokens))
        } else if is_comparison(&binary.op) {
            let borrowed = self.after_records(&right.pending, quote!(&(#right_tokens)));
            quote!(*#borrowed)
        } else {
            self.after_records(&right.pending, quote!((#right_tokens)))
        };
        let span = located(binary.op.span());
        let tokens = quote_spanned!(span=> (#left_tokens) #operator #right_tokens);
        self.record_value(mode, column, start, tokens, &left.pending, span)
    }

    fn lower_unary(&mut self, unary: &ExprUnary, mode: Mode) -> Lowered {
        let start = self.begin();
        let column = self.column();
        self.push(&compact(unary.op.to_token_stream()));
        let inner = self.lower(&unary.expr, mode);
        let operator = &unary.op;
        let inner_tokens = inner.tokens;
        let span = located(unary.op.span());
        let tokens = quote_spanned!(span=> #operator (#inner_tokens));
        if matches!(unary.op, UnOp::Deref(_)) || is_literal(&unary.expr) {
            Lowered {
                tokens,
                pending: inner.pending,
                purity: inner.purity,
            }
        } else {
            self.record_value(mode, column, start, tokens, &inner.pending, span)
        }
    }

    fn lower_range(&mut self, range: &ExprRange, mode: Mode) -> Lowered {
        let from = range.start.as_ref().map(|from| self.lower(from, mode));
        let limits = &range.limits;
        self.push(&compact(limits.to_token_stream()));
        let to = range.end.as_ref().map(|to| self.lower(to, mode));
        let mut pending = Vec::new();
        let mut purity = Purity::Pure;
        let mut side = |lowered: Option<Lowered>| {
            lowered.map(|lowered| {
                pending.extend(lowered.pending);
                purity = combine(purity, lowered.purity);
                let tokens = lowered.tokens;
                quote!((#tokens))
            })
        };
        let (from_tokens, to_tokens) = (side(from), side(to));
        Lowered {
            tokens: quote!(#from_tokens #limits #to_tokens),
            pending,
            purity,
        }
    }

    fn lower_struct(&mut self, structure: &ExprStruct, mode: Mode) -> Lowered {
        let path = ExprPath {
            attrs: Vec::new(),
            qself: structure.qself.clone(),
            path: structure.path.clone(),
        };
        self.push(&compact(path.to_token_stream()));
        self.push(" {");
        let mut fields = Vec::new();
        let mut pending = Vec::new();
        for (index, field) in structure.fields.iter().enumerate() {
            self.push(if index == 0 { " " } else { ", " });
            if field.colon_token.is_some() {
                self.push(&member_text(&field.member));
                self.push(": ");
            }
            let lowered = self.lower(&field.expr, mode);
            let (member, tokens) = (&field.member, lowered.tokens);
            fields.push(quote!(#member: (#tokens)));
            pending.extend(lowered.pending);
        }
        let rest = structure.rest.as_ref().map(|rest| {
            self.push(if structure.fields.is_empty() {
                " .."
            } else {
                ", .."
            });
            let lowered = self.lower(rest, mode);
            pending.extend(lowered.pending);
            let tokens = lowered.tokens;
            quote!(..(#tokens))
        });
        self.push(" }");
        let separator = (!fields.is_empty() && rest.is_some()).then(|| quote!(,));
        Lowered::impure_with(quote!(#path { #(#fields),* #separator #rest }), pending)
    }

    /// `matches!` is walked; a macro known to produce a value is recorded
    /// whole; any other macro is printed and left as written, because it
    /// may expand to a place the expression must not move.
    fn lower_macro(&mut self, macro_call: &ExprMacro, mode: Mode) -> Lowered {
        let start = self.begin();
        let column = self.column();
        let span = located(macro_call.mac.bang_token.span());
        if let Some(args) = matches_args(&macro_call.mac) {
            let path = &macro_call.mac.path;
            self.push(&compact(path.to_token_stream()));
            self.push("!(");
            let scrutinee = self.lower(&args.scrutinee, mode);
            self.push(", ");
            self.push(&pattern_text(&args.pattern));
            if let Some(guard) = &args.guard {
                self.push(" if ");
                self.push(&compact(guard.to_token_stream()));
            }
            self.push(")");
            let (pattern, guard) = (&args.pattern, args.guard.as_ref().map(|g| quote!(if #g)));
            let scrutinee_tokens = scrutinee.tokens;
            let tokens = quote_spanned!(span=> #path!((#scrutinee_tokens), #pattern #guard));
            return self.record_value(mode, column, start, tokens, &scrutinee.pending, span);
        }
        self.push(&compact(macro_call.to_token_stream()));
        let tokens = macro_call.to_token_stream();
        if produces_value(&macro_call.mac) {
            self.record_value(mode, column, start, tokens, &[], span)
        } else {
            Lowered::impure(tokens)
        }
    }

    fn lower_closure(&mut self, closure: &ExprClosure) -> Lowered {
        if let Some(lifetimes) = &closure.lifetimes {
            self.push(&compact(lifetimes.to_token_stream()));
            self.push(" ");
        }
        for keyword in [
            closure.constness.map(|_| "const "),
            closure.movability.map(|_| "static "),
            closure.asyncness.map(|_| "async "),
            closure.capture.map(|_| "move "),
        ]
        .into_iter()
        .flatten()
        {
            self.push(keyword);
        }
        self.push("|");
        for (index, input) in closure.inputs.iter().enumerate() {
            if index > 0 {
                self.push(", ");
            }
            self.push(&compact(input.to_token_stream()));
        }
        self.push("|");
        if let ReturnType::Type(_, ty) = &closure.output {
            self.push(" -> ");
            self.push(&compact(ty.to_token_stream()));
        }
        self.push(" ");
        self.lower(&closure.body, Mode::PrintOnly);
        Lowered::impure(closure.to_token_stream())
    }
}

impl Lowered {
    /// A container of lowered parts: a tuple, array or struct literal. It is
    /// impure because building it again would move its parts again.
    fn impure_with(tokens: TokenStream, pending: Vec<Pending>) -> Self {
        Self {
            tokens,
            pending,
            purity: Purity::Impure,
        }
    }
}

fn combine(left: Purity, right: Purity) -> Purity {
    if left == Purity::Pure && right == Purity::Pure {
        Purity::Pure
    } else {
        Purity::Impure
    }
}

fn is_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(_))
}

fn is_comparison(operator: &BinOp) -> bool {
    matches!(
        operator,
        BinOp::Eq(_) | BinOp::Ne(_) | BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_)
    )
}

fn member_text(member: &Member) -> String {
    match member {
        Member::Named(ident) => ident.to_string(),
        Member::Unnamed(index) => index.index.to_string(),
    }
}

/// `| A | B` prints as `A | B`; `compact` would read the leading `|` as a
/// closure's.
fn pattern_text(pattern: &Pat) -> String {
    match pattern {
        Pat::Or(alternatives) => alternatives
            .cases
            .iter()
            .map(|case| compact(case.to_token_stream()))
            .collect::<Vec<_>>()
            .join(" | "),
        other => compact(other.to_token_stream()),
    }
}

/// Byte offset of the last segment's name in the printed `text`.
fn last_segment_offset(path: &ExprPath, text: &str) -> usize {
    let single_segment =
        path.qself.is_none() && path.path.leading_colon.is_none() && path.path.segments.len() == 1;
    if single_segment {
        return 0;
    }
    let name = path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
    text.rmatch_indices("::")
        .map(|(offset, separator)| offset + separator.len())
        .find(|&offset| text[offset..].starts_with(&name))
        .unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathKind {
    /// A single lowercase segment: a local variable, recorded by reference.
    Local,
    /// A `SCREAMING_CASE` last segment: a constant, recorded by reference.
    Constant,
    /// A capitalized last segment: a unit struct or variant, whose value
    /// the source already shows.
    Constructor,
    /// A lowercase last segment with a prefix: a function item.
    Function,
}

fn classify(path: &ExprPath) -> PathKind {
    let last = path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
    let single_segment =
        path.qself.is_none() && path.path.leading_colon.is_none() && path.path.segments.len() == 1;
    if last.chars().any(|c| c.is_ascii_uppercase()) && !last.chars().any(char::is_lowercase) {
        PathKind::Constant
    } else if last.starts_with(char::is_uppercase) {
        PathKind::Constructor
    } else if single_segment {
        PathKind::Local
    } else {
        PathKind::Function
    }
}

/// Macros whose expansion is a value, which the expression may move.
const VALUE_MACROS: &[&str] = &[
    "cfg",
    "column",
    "concat",
    "env",
    "file",
    "format",
    "include_bytes",
    "include_str",
    "json",
    "line",
    "module_path",
    "option_env",
    "stringify",
    "vec",
];

fn produces_value(macro_call: &Macro) -> bool {
    macro_call
        .path
        .segments
        .last()
        .is_some_and(|segment| VALUE_MACROS.contains(&segment.ident.to_string().as_str()))
}

struct MatchesArgs {
    scrutinee: Expr,
    pattern: Pat,
    guard: Option<Expr>,
}

impl Parse for MatchesArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let scrutinee = input.parse()?;
        input.parse::<Token![,]>()?;
        let pattern = Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(Token![if]) {
            input.parse::<Token![if]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        input.parse::<Option<Token![,]>>()?;
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the pattern"));
        }
        Ok(Self {
            scrutinee,
            pattern,
            guard,
        })
    }
}

/// The arguments of `matches!`, `std::matches!` or `core::matches!`.
fn matches_args(macro_call: &Macro) -> Option<MatchesArgs> {
    let segments: Vec<String> = macro_call
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let is_matches = match segments.as_slice() {
        [name] => name == "matches",
        [krate, name] => (krate == "std" || krate == "core") && name == "matches",
        _ => false,
    };
    if !is_matches {
        return None;
    }
    syn::parse2(macro_call.tokens.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The printed text and each slot as `(column, text of the subexpression)`,
    /// in column order.
    fn lowered(source: &str) -> (String, Vec<(usize, String)>) {
        let expr: Expr = syn::parse_str(source).expect("test source parses");
        let mut lowering = Lowering::new(hygienic("__recorder"));
        lowering.lower(&expr, Mode::Record);
        let (text, slots) = lowering.finish();
        let mut recorded: Vec<(usize, String)> = slots
            .iter()
            .map(|slot| (slot.column, text[slot.start..slot.end].to_owned()))
            .collect();
        recorded.sort();
        (text, recorded)
    }

    fn recorded(entries: &[(usize, &str)]) -> Vec<(usize, String)> {
        entries
            .iter()
            .map(|(column, text)| (*column, (*text).to_owned()))
            .collect()
    }

    fn tokens_of(source: &str) -> String {
        let expr: Expr = syn::parse_str(source).expect("test source parses");
        let mut lowering = Lowering::new(hygienic("__recorder"));
        let lowered = lowering.lower(&expr, Mode::Record);
        lowering
            .after_records(&lowered.pending, lowered.tokens)
            .to_string()
    }

    #[test]
    fn operators_are_recorded_at_the_operator_and_operands_at_their_start() {
        let (text, slots) = lowered("price * quantity + shipping == expected");
        assert_eq!(text, "price * quantity + shipping == expected");
        assert_eq!(
            slots,
            recorded(&[
                (0, "price"),
                (6, "price * quantity"),
                (8, "quantity"),
                (17, "price * quantity + shipping"),
                (19, "shipping"),
                (28, "price * quantity + shipping == expected"),
                (31, "expected"),
            ])
        );
    }

    #[test]
    fn method_calls_are_recorded_at_the_method_name() {
        let (text, slots) = lowered("scores.iter().sum::<i32>()");
        assert_eq!(text, "scores.iter().sum::<i32>()");
        assert_eq!(
            slots,
            recorded(&[
                (0, "scores"),
                (7, "scores.iter()"),
                (14, "scores.iter().sum::<i32>()"),
            ])
        );
    }

    #[test]
    fn field_chain_records_every_step() {
        let (_, slots) = lowered("order.customer.name");
        assert_eq!(
            slots,
            recorded(&[
                (0, "order"),
                (6, "order.customer"),
                (15, "order.customer.name")
            ])
        );
    }

    #[test]
    fn function_call_is_recorded_at_its_name_and_constructors_are_not() {
        let (_, slots) = lowered("total(Some(x)) == None");
        assert_eq!(
            slots,
            recorded(&[
                (0, "total(Some(x))"),
                (11, "x"),
                (15, "total(Some(x)) == None")
            ])
        );
    }

    #[test]
    fn qualified_function_is_recorded_at_its_last_segment() {
        let (_, slots) = lowered("std::cmp::max(a, b)");
        assert_eq!(
            slots,
            recorded(&[(10, "std::cmp::max(a, b)"), (14, "a"), (17, "b")])
        );
    }

    #[test]
    fn generic_function_is_recorded_at_its_name_not_its_type_argument() {
        let (text, slots) = lowered("u::<u8>(3) == 4");
        assert_eq!(text, "u::<u8>(3) == 4");
        assert_eq!(
            slots,
            recorded(&[(0, "u::<u8>(3)"), (11, "u::<u8>(3) == 4")])
        );
        let (_, slots) = lowered("a::b::<T>(x)");
        assert_eq!(slots, recorded(&[(3, "a::b::<T>(x)"), (10, "x")]));
    }

    #[test]
    fn constants_are_recorded_and_unit_variants_are_not() {
        let (_, slots) = lowered("i32::MAX > LIMIT && Ordering::Less == x");
        assert_eq!(
            slots,
            recorded(&[
                (5, "i32::MAX"),
                (9, "i32::MAX > LIMIT"),
                (11, "LIMIT"),
                (17, "i32::MAX > LIMIT && Ordering::Less == x"),
                (35, "Ordering::Less == x"),
                (38, "x"),
            ])
        );
    }

    #[test]
    fn index_records_base_subscript_and_element() {
        let (_, slots) = lowered("rows[i + 1]");
        assert_eq!(
            slots,
            recorded(&[(0, "rows"), (4, "rows[i + 1]"), (5, "i"), (7, "i + 1")])
        );
    }

    #[test]
    fn element_is_recorded_through_a_reference_so_index_runs_once() {
        let tokens = tokens_of("rows[0] == 1");
        assert_eq!(tokens.matches("[0]").count(), 1, "{tokens}");
        assert!(tokens.contains("* match"), "{tokens}");
    }

    #[test]
    fn unary_operators_are_recorded_except_deref_and_negated_literals() {
        let (_, slots) = lowered("!done && -1 < *count");
        assert_eq!(
            slots,
            recorded(&[
                (0, "!done"),
                (1, "done"),
                (6, "!done && -1 < *count"),
                (12, "-1 < *count"),
                (15, "count"),
            ])
        );
    }

    #[test]
    fn literals_are_not_recorded() {
        let (_, slots) = lowered("1 + 2 == 3");
        assert_eq!(slots, recorded(&[(2, "1 + 2"), (6, "1 + 2 == 3")]));
    }

    #[test]
    fn closures_are_printed_but_not_walked() {
        let (text, slots) = lowered("items.iter().any(|item| item.len() > 3)");
        assert_eq!(text, "items.iter().any(|item| item.len() > 3)");
        assert_eq!(
            slots,
            recorded(&[
                (0, "items"),
                (6, "items.iter()"),
                (13, "items.iter().any(|item| item.len() > 3)"),
            ])
        );
    }

    #[test]
    fn matches_macro_records_its_scrutinee() {
        let (text, slots) = lowered("matches!(outcome, Ok(n) if n > 1)");
        assert_eq!(text, "matches!(outcome, Ok(n) if n > 1)");
        assert_eq!(
            slots,
            recorded(&[(0, "matches!(outcome, Ok(n) if n > 1)"), (9, "outcome")])
        );
    }

    #[test]
    fn matches_pattern_with_a_leading_bar_prints_without_it() {
        let (text, _) = lowered("matches!(x, | None | Some(1))");
        assert_eq!(text, "matches!(x, None | Some(1))");
    }

    #[test]
    fn value_macros_are_recorded_whole_and_other_macros_are_left_as_written() {
        let (text, slots) = lowered("format!(\"{a}-{b}\") == expected");
        assert_eq!(text, "format!(\"{a}-{b}\") == expected");
        assert_eq!(
            slots,
            recorded(&[
                (0, "format!(\"{a}-{b}\")"),
                (19, "format!(\"{a}-{b}\") == expected"),
                (22, "expected"),
            ])
        );
        let (text, slots) = lowered("name_of!(user) == \"Ada\"");
        assert_eq!(text, "name_of!(user) == \"Ada\"");
        assert_eq!(slots, recorded(&[(15, "name_of!(user) == \"Ada\"")]));
    }

    #[test]
    fn cast_try_and_await_are_recorded_at_their_operator() {
        let (_, slots) = lowered("count as u64");
        assert_eq!(slots, recorded(&[(0, "count"), (6, "count as u64")]));
        let (_, slots) = lowered("parse(text)?");
        assert_eq!(
            slots,
            recorded(&[(0, "parse(text)"), (6, "text"), (11, "parse(text)?")])
        );
        let (_, slots) = lowered("fetch().await");
        assert_eq!(slots, recorded(&[(0, "fetch()"), (8, "fetch().await")]));
    }

    #[test]
    fn borrows_record_the_borrowed_place() {
        let (text, slots) = lowered("sum(&mut totals, &base)");
        assert_eq!(text, "sum(&mut totals, &base)");
        assert_eq!(
            slots,
            recorded(&[(0, "sum(&mut totals, &base)"), (9, "totals"), (18, "base")])
        );
    }

    #[test]
    fn struct_literal_records_field_values_with_shorthand_kept() {
        let (text, slots) = lowered("Point { x, y: y0 + 1, ..base } == expected");
        assert_eq!(text, "Point { x, y: y0 + 1, ..base } == expected");
        assert_eq!(
            slots,
            recorded(&[
                (8, "x"),
                (14, "y0"),
                (17, "y0 + 1"),
                (24, "base"),
                (31, "Point { x, y: y0 + 1, ..base } == expected"),
                (34, "expected"),
            ])
        );
    }

    #[test]
    fn repeat_length_is_printed_without_a_record() {
        let (text, slots) = lowered("[0u8; 2 * 2] == buffer");
        assert_eq!(text, "[0u8; 2 * 2] == buffer");
        assert_eq!(
            slots,
            recorded(&[(13, "[0u8; 2 * 2] == buffer"), (16, "buffer")])
        );
    }

    #[test]
    fn multi_line_source_prints_on_one_line() {
        let (text, _) = lowered("first\n    .len()\n    == 3");
        assert_eq!(text, "first.len() == 3");
    }

    #[test]
    fn right_operand_places_are_recorded_after_the_left_operand() {
        for source in ["bump(&mut x) != x", "a && b", "step() + x"] {
            let tokens = tokens_of(source);
            let operator = tokens
                .find(['!', '&', '+'])
                .expect("the operator is emitted");
            let record_of_right = tokens.rfind("record").expect("the right place is recorded");
            assert!(record_of_right > operator, "{source}: {tokens}");
        }
    }

    #[test]
    fn call_free_expressions_read_only_places_and_operators() {
        let call_free = |source: &str| is_call_free(&syn::parse_str(source).expect("parses"));
        assert!(call_free("values[i % n] < limit && !done"));
        assert!(call_free("(a, b) == (1, 2)"));
        assert!(call_free("Point { x, ..base }.x as u64 - 1"));
        assert!(!call_free("values.len() < limit"));
        assert!(!call_free("total(x) == 1"));
        assert!(!call_free("vec![1] == x"));
        assert!(!call_free("parse(x)? == 1"));
        assert!(!call_free("matches!(x, Some(_))"));
    }
}
