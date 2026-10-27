# Development

## Setup and checks

assertoscope uses [mise](https://mise.jdx.dev) to install its development tools
and to run its tasks. Install mise first; it provides everything else. CI
runs the same tasks, so `mise run check` on your machine runs the check CI
runs.

- `mise.toml` specifies each tool and its version, including the Rust
  toolchain.
- `mise.lock` records each tool's download URL and checksum for every
  platform.

Then install the tools and run the checks:

```sh
mise install
mise run check
```

`mise run check` runs these tasks in parallel and does not rewrite files. Run
one alone with `mise run <task>`.

| Task                  | Fails when                                                                           |
|-----------------------|--------------------------------------------------------------------------------------|
| `check-toolchain-pin` | `rust-toolchain.toml` and `mise.lock` specify a different Rust version or components |
| `check-scripts`       | a bash task or script has a syntax error                                             |
| `format-check`        | Rust code is not formatted                                                           |
| `clippy`              | Clippy reports any warning                                                           |
| `test`                | a test fails                                                                         |
| `build`               | a package does not build                                                             |

## Tests

Tests are fast, isolated and repeatable.

### Test types

| Type               | Where                                                                                                   | Runs            |
|--------------------|---------------------------------------------------------------------------------------------------------|-----------------|
| Unit tests         | `#[cfg(test)]` modules under `src/` and `macros/src/`                                                   | `mise run test` |
| Integration tests  | `tests/`: the macros as a user calls them                                                               | `mise run test` |
| Compile-fail tests | `tests/compile_fail/`: an expression the macros reject, with the error it must report in the `.stderr` beside it | `mise run test` |

- Unit tests are sociable: each tests behaviour
  through real pieces together, not through test doubles of its
  collaborators.
- A macro's behaviour is tested under `tests/`. The expansion refers to
  `::assertoscope`, a path that does not resolve inside the crate's own
  unit tests. `tests/readme.rs` runs each README example and fails when the
  README's text block differs from what the library prints.
- A compile-fail test's `.stderr` records the compiler's output for the
  pinned toolchain. After a toolchain bump, or a change to the error the
  macros produce, regenerate it with `TRYBUILD=overwrite mise run test` and
  review the diff.

### Test first, whenever reasonable

- A bug fix starts with a test that fails on the bug (red), then the fix
  makes it pass (green).
- A new feature or other change starts with its test when the new behaviour
  can be expressed as a test up front.
- While exploring an implementation direction, or writing something new
  without an obvious test structure, write the tests once the design
  settles. Tests written that early cost time and constrain a design that is
  still changing.

### Real pieces, test doubles at the edges

- Use test doubles at the system's edges: the clock, the terminal, the
  network.
- Do not replace any other internal collaborator with a test double: each
  piece's tests would pass while no test runs the two pieces together.

### Mutation testing

A good test fails when the logic it covers changes in any way. Check a test
by breaking that logic, such as inverting a condition, removing a step or
moving a boundary by one, and running the test again. A test that still
passes does not cover that logic.

### Test-only fields

- Observe behaviour through effects the code already has: what the cache
  stores, sentinel text in a fixture, what a test double at the edge
  recorded.
- Add a `#[cfg(test)]` field, counter or accessor to a production type only
  when every alternative costs far more. The pull request states the reason,
  reviewers check it, and a comment beside the field explains why it exists.

### Names and messages

- Test names do not start with an article: `string_value_renders_indented`,
  not `a_string_value_renders_indented`. Write `single` or `multiple` when
  the claim depends on quantity.
- A test name that states what does not happen uses `doesnt`, or `dont`
  after a plural subject.
- Assertion messages state the expectation the assertion checks.

## Commits

- A small change is one commit. A larger change is a series of logical
  commits; each message explains its part of the design.
- Put a refactoring, such as a rename, in a commit of its own, and build the
  change on it in the next. Reviewers can then check that the refactoring
  does not change behaviour, and read the logic change alone.
- Squash review changes and corrections into the commit that added the code
  they change: `git commit --fixup=<commit>`, then
  `git rebase --autosquash <base>`. The history then shows each commit as
  written, without review commits, and blame points at the commit that
  explains the code.
- Commit messages:
  - The subject is imperative and names the behaviour that changed, not the
    files or symbols.
  - The body opens with the changed behaviour or result, then the defect,
    constraint or tradeoff that required it.
  - Leave out what the diff shows. The length tracks the change: a small
    fix may need only the subject.
  - Name the tests the commit adds and the behaviour they protect, not that
    existing tests still pass.

## Pull requests

- A pull request changes what its task needs. Leave what you notice along
  the way, such as an existing US spelling, for a change of its own:
  unrelated edits enlarge the diff a reviewer reads.
- The title names the user-visible effect, not the mechanism. A change
  without a user-visible effect starts its title with `(internal) `.
- The body uses these headings, in order:
  1. **The bug**, or **The change** for a feature: the user-visible problem
     or effect.
  2. **After this change**: what the user sees now.
  3. The cause, then the fix, each under its own heading.
  4. **Validation**: two to four lines naming the groups of behaviour the
     tests cover, not individual tests.
- `mise run check` passes.

## Changelog

- Add an entry under `## Unreleased` at the top of `CHANGELOG.md`, creating
  the heading when the last release retitled it. A release retitles that
  heading with its version and date, and refuses to run unless exactly one
  exists; see [RELEASE.md](RELEASE.md).
- Group entries under `### Security`, `### Enhancements`, `### Fixes` and
  `### Changes`. `### Changes` holds a behaviour change that is neither a
  fix nor an enhancement.
- Group work without a user-visible effect under `### Internal: <area>`.
- A fix starts with the verb and the symptom: "Fix …". A speedup is an
  enhancement, stated as a ratio.
- An entry under `## Unreleased` describes the net behaviour. A later change
  to an unreleased feature or fix updates that entry instead of adding its
  own.
- Do not add an entry for a CI or lint gate.

## Workflows

- **Releasing assertoscope:** [RELEASE.md](RELEASE.md) covers the changelog,
  the version bump and publishing.
