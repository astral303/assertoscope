## Unreleased

### Enhancements

- Add `assert!`, `assert_eq!` and `assert_ne!`, and their `debug_` variants.
  On failure the panic message draws the expression with the value of each
  local, constant, field, element, call and operator under it, as Groovy's
  power assertions do.
  - `&&` and `||` show only the operands that ran.
  - A value too wide for the diagram is numbered and listed below it; a
    string with line breaks is listed as text.
  - `matches!` shows the value that did not match.
  - A value without `Debug` is left out instead of failing to compile, so
    `assert_eq!` accepts values the standard macro rejects.
