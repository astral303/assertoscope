# assertoscope

**See the values behind the failure.**

Assertoscope brings Groovy-style power assertions to Rust. Write an ordinary
expression in `assert!`; when it fails, the value of each local, field,
element, call and operator is shown under the expression.

```rust
use assertoscope::assert;

let price = 12;
let quantity = 3;
let shipping = 5;
let expected = 40;

assert!(price * quantity + shipping == expected);
```

```text
assertion failed

    price * quantity + shipping == expected
    |     | |        | |        |  |
    12    | 3        | 5        |  40
          36         41         false
```

The same holds for `assert_eq!` and `assert_ne!`, and for the `debug_assert!`
family.

## Getting started

Add Assertoscope as a development dependency:

```sh
cargo add --dev assertoscope
```

Import the macros in your test module. The import shadows the standard macros
in that scope: existing assertions keep their meaning and start showing their
values. A few expressions do not compile; see [Limitations](#limitations).

```rust
#[cfg(test)]
mod tests {
    use assertoscope::{assert, assert_eq, assert_ne};

    #[test]
    fn total_includes_shipping() {
        let prices = [12, 8, 5];
        let shipping = 5;

        assert!(prices.iter().sum::<i32>() + shipping == 30);
    }
}
```

## Assertions that show their work

Each value is rendered with `Debug` and placed under the token it belongs to:

- A local or constant appears under its name; a field or element under the
  field name or the `[`.
- A method or function call appears under its name, with its result.
- An operator, cast, `?` or `.await` appears under the operator, with its
  result.

A method chain shows the receiver and every step. A closure is printed as
written:

```rust
let words = ["alpha", "beta"];

assert!(words.iter().all(|word| word.len() > 4));
```

```text
assertion failed

    words.iter().all(|word| word.len() > 4)
    |     |      |
    |     |      false
    |     Iter(["alpha", "beta"])
    ["alpha", "beta"]
```

A field or index chain shows each step, so the element that failed the check
is visible next to the collection it came from:

```rust
let customer = Customer {
    name: "Ada".to_owned(),
    orders: vec![Order { id: 7, total: 41 }, Order { id: 8, total: 12 }],
};

assert!(customer.orders[1].total > 20);
```

```text
assertion failed

    customer.orders[1].total > 20
    |        |     |   |     |
    #1       |     |   12    false
             |     Order { id: 8, total: 12 }
             [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }]

    #1 customer = Customer { name: "Ada", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }
```

`&&` and `||` short-circuit as usual, and the diagram shows only what ran:

```rust
let items: Vec<u32> = Vec::new();

assert!(!items.is_empty() && items[0] > 1);
```

```text
assertion failed

    !items.is_empty() && items[0] > 1
    ||     |          |
    |[]    true       false
    false
```

### `assert_eq!` and `assert_ne!`

The two sides are shown as one comparison, with the values inside each side:

```rust
assert_eq!(customer.orders.len(), 3);
```

```text
assertion failed

    customer.orders.len() == 3
    |        |      |     |
    #1       |      2     false
             [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }]

    #1 customer = Customer { name: "Ada", orders: [Order { id: 7, total: 41 }, Order { id: 8, total: 12 }] }
```

Unlike the standard macros, the compared values need not implement `Debug`;
a value without it is left out of the diagram.

### `matches!`

`matches!` is the one macro whose first argument is shown with its value, so
the diagram includes the value that did not match:

```rust
let outcome: Result<u32, String> = Err("timeout".to_owned());

assert!(matches!(outcome, Ok(n) if n > 1));
```

```text
assertion failed

    matches!(outcome, Ok(n) if n > 1)
    |        |
    false    Err("timeout")
```

### Long values are listed below the diagram

A value wider than 60 characters, or a string with line breaks, is replaced
by a number in the diagram and listed under it. A multi-line string is listed
as text, so there is no need to repeat it in a message. Carriage returns,
tabs and control characters stay escaped in that text, so a `\r\n` mismatch
is visible:

```rust
let output = "listed=3\nwarnings=0\n";

assert!(output.contains("listed=4\n"));
```

```text
assertion failed

    output.contains("listed=4\n")
    |      |
    #1     false

    #1 output =
        listed=3
        warnings=0
```

A rendering longer than 16 KiB is cut and ends with `…`.

### Add context when it helps

An optional message carries context from outside the expression, such as
which case is being checked. The values are still shown:

```rust
let order_id = "A-17";
let actual_total = 41;
let quoted_total = 40;

assert!(
    actual_total == quoted_total,
    "order {order_id} should match its quote",
);
```

```text
assertion failed: order A-17 should match its quote

    actual_total == quoted_total
    |            |  |
    41           |  40
                 false
```

## Evaluation

- A call runs once, in the order it would without the macro.
- A condition without a call, such as `values[i] < limit`, runs as written.
  When it fails, its locals, fields and elements are read again to collect
  the values, so a passing assertion of that shape costs what the standard
  macro costs.
- A condition with a call renders each value with `Debug` as the call
  produces it, whether the assertion passes or fails, up to 16 KiB per
  value. An assertion that calls a method on a large collection inside a
  loop pays that rendering on every pass.
- A local, field or constant is read by reference just before the expression
  that uses it runs. Moving it, mutating it or borrowing it mutably works as
  usual: `assert!(stack.pop() == Some(3))` and `assert!(consume(text) == 4)`
  both compile and show the value.
- A call's result is shown as it is produced, then passed on. Temporaries
  live until the condition has been evaluated, so
  `assert!(cell.borrow().len() == 1)` compiles.
- A passing assertion returns `()`. A failing one panics with the diagram as
  its message, located at the assertion. The message is formatted only on
  failure, as with the standard macros.

## Limitations

- A value whose type has no `Debug` implementation is left out of the
  diagram. In a generic function, add a `Debug` bound to show the value. A
  value whose type is inferred from code after the assertion, such as
  `Default::default()`, must implement `Debug`.
- Closures, blocks, `if`, `match` and the arguments of macros other than
  `matches!` are printed as written; only their result is shown. A macro
  other than `vec!`, `format!`, `json!` and the standard literal macros is
  printed as written and its result is not shown.
- A field or element of a call result cannot be moved out of or mutated;
  see below.
- These do not compile inside an assertion: a `#[repr(packed)]` field, a
  `static mut`, and the assertion itself in a `const fn`.
- The crate must be depended on under its own name: the macros expand to
  paths that start with `::assertoscope`.
- Clippy lints that skip macro code, such as `cast_possible_truncation`, do
  not fire inside a condition with a call. A condition without a call is
  linted as with the standard `assert!`.
- Values are aligned by character count, so a value containing wide
  characters shifts the values to its right.

### A field of a call result is read through a reference

`fetch().body` is shown by borrowing the field of the temporary that
`fetch()` returned, which leaves a place behind a shared reference:

| Use of `fetch().body`                     | Compiles |
|-------------------------------------------|----------|
| compare, read, copy, `&self` method call  | yes      |
| move out, such as `unwrap()`, `into_iter()` | no     |
| `&mut self` method call, such as `pop()`  | no       |

```rust
assert!(fetch().body.unwrap() == "text");
```

```text
error[E0507]: cannot move out of a shared reference
  --> tests/compile_fail/field_of_call_result_moved.rs:19:21
   |
19 |     assert!(fetch().body.unwrap() == "text");
   |                     ^^^^ ------ value moved due to this method call
   |                     |
   |                     move occurs because value has type `Option<String>`, which does not implement the `Copy` trait
```

Borrow instead of moving, or bind the call result to a local first:

```rust
assert!(fetch().body.as_deref() == Some("text"));

let report = fetch();
assert!(report.body.unwrap() == "text");
```

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for toolchain setup, checks, and
contribution guidelines.

## License

[MIT](LICENSE).
