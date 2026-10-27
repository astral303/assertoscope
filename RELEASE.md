# Releasing

Requires a [crates.io](https://crates.io) token (`cargo login`).
Uses [cargo-release](https://github.com/crate-ci/cargo-release), provided
by `mise install`. 

1. Write the changelog entry. (The release replaces the `## Unreleased`
   heading in `CHANGELOG.md` with the version and date, and refuses to run unless
   exactly one such heading exists.)

2. Preview, then release (default `<bump>` is `patch`):

   ```bash
   mise run release-preview [<bump>]   # preview the steps; --no-verify skips the build
   mise run release-execute [<bump>]   # bump, commit, publish, tag, push
   ```

A release:

* Checks that the tree is clean, the branch is `main`, and `origin` is not
  ahead. 
* Bumps the version in `Cargo.toml` and `Cargo.lock` and retitles the changelog
  heading.
* Commits `release vX.Y.Z`, publishes to crates.io, tags `vX.Y.Z`, and pushes.

