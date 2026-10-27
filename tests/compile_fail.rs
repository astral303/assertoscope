//! Each expression under `tests/compile_fail/` must fail to compile with the
//! error recorded in the `.stderr` beside it. The README quotes that error.

#[test]
fn unsupported_expressions_report_the_recorded_error() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/*.rs");
}
