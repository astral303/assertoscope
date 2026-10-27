# Roadmap

Candidate changes after the first release. Within each section, items are
in rough priority order. When an item ships, it moves to CHANGELOG.md.

## Output

- Pretty-printed legend (the numbered list of wide values below the
  diagram): render each entry with `{:#?}`, so a struct or collection wider
  than the diagram is printed across several lines instead of one. Requires
  recording a second rendering per value, or deriving the compact form from
  the pretty one.
- Marker for values without `Debug`: show a marker for a value left out, so
  a reader can tell "not shown" from "not evaluated". Needs the `Recorder`
  to distinguish the two; today both are `None`.
- Diff for long `assert_eq!` strings, in the style of `pretty_assertions`,
  below the diagram.
- Width-aware alignment with `unicode-width`, so values containing wide
  characters do not shift the values to their right.
- Configurable `MAX_INLINE_WIDTH` (60 characters) and `MAX_CAPTURE_BYTES`
  (16 KiB), through an environment variable or a crate feature, once a
  large test suite shows which limit is too small.

## Evaluation cost

- Defer rendering for the collection an assertion indexes and for comparison
  operands: hold a reference until the outcome is known and render on
  failure only. A passing `assert!(values[i].is_some())` in a loop would
  then cost what the standard macro costs; today it renders `values` on
  every run, up to `MAX_CAPTURE_BYTES`. The reference would forbid mutating
  the collection in the same assertion.

## Expressions

- Clippy lints for a condition with a call: the standard `assert!` reports
  `cast_possible_truncation` on `items.len() as u8`; assertoscope's
  `assert!` does not.
  - Cause: once a condition contains a call, each value expression is
    wrapped in a generated `match`, and the whole condition carries the
    macro's syntax context. Clippy lints that skip macro code do not fire
    on it. A condition without a call is emitted as written and is linted.
  - Candidate fix: statement-form lowering, one `let` per recorded value:
    `let v1 = items.len(); record(&v1); let v2 = v1 as u8; ...`. A compile
    error would then point at the whole subexpression, where today it
    points at one token.
  - Cost: the temporaries would need the user's syntax context and lose
    macro hygiene. A temporary could collide with a local of the same name
    in the user's code; a prefix such as `__assertoscope_` makes that name
    unlikely there.
- Record values inside closures per invocation, shown as a list under the
  closure body, so `items.iter().all(|i| i.valid)` shows which element
  failed. Needs a `Recorder` the closure can borrow. A `'static` `move`
  closure cannot borrow it; its body would stay printed as written.
- Show a local passed to `vec!`, `format!`, `json!` or a standard literal
  macro. Today their arguments are printed as written.

## New macros

- `assert_matches!`, and a non-panicking `check!`: it reports all failures
  of a test at its end.

## Documentation and tooling

- Generate README examples from the tests. An `xtask` runs each example in
  `tests/readme.rs` and splices its code and output between markers in
  README.md. A test fails when the `xtask` would change the file. The
  README's code and the test's code are then one copy; today they are
  maintained by hand and only the output is verified.
- Build at the declared minimum Rust version in CI; `rust-version` is
  stated but not checked.
