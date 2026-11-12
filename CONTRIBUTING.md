# Contributing to Caliper

Thanks for your interest in Caliper — a terminal-native, AI-assisted parametric
CAD toolkit in pure Rust. Contributions of all sizes are welcome.

## Before you start

By contributing you agree to two things:

1. **The Contributor License Agreement ([`CLA.md`](CLA.md)).** It lets you keep
   ownership of your work while granting the Project the licenses it needs to
   ship Caliper as open source *and* to offer commercial/hosted editions on top.
2. **The Developer Certificate of Origin (DCO).** Sign off every commit with
   `git commit -s`, which adds a `Signed-off-by: Your Name <you@example.com>`
   trailer certifying you wrote the change or have the right to submit it under
   the project license. The DCO text is at <https://developercertificate.org/>.

A CLA-assistant check may ask you to click to agree on your first pull request.

## Project principles (please preserve these)

Caliper has deliberate, load-bearing constraints. Changes that break them will
be asked to find another way:

- **The kernel (`crates/cad-core`) is `std`-only — zero third-party crates.**
  The CLI (`crates/cad-cli`) is zero-crate too. External *programs* are allowed
  only "at the edges" (`curl`/`jq` for the AI transport, `stty` for the TUI).
- **Everything is text.** A design is a `.cad` script — scriptable, diffable,
  reproducible. New features should keep that property.
- **Geometry is exact where it can be.** Prefer integer-exact / robust
  predicates over floating-point fudging.

## Development workflow

```sh
cargo test --workspace                                   # all tests must pass
cargo fmt --all --check                                  # formatting
cargo clippy --all-targets --all-features -- -D warnings # no warnings
```

All three must be green before a PR merges. Add tests for new behavior
(the kernel is heavily unit- and integration-tested; see `crates/cad-core/tests`).

1. Fork and branch from `main`.
2. Make your change with tests and docs (update `reference.txt`, `docs/`,
   and `CHANGELOG.md` where relevant).
3. Run the three checks above.
4. Open a pull request with a clear description. Sign off your commits (`-s`).

## Licensing of your contributions

Caliper is **open core**. The code in this repository is licensed under
Apache-2.0 (see [`LICENSE`](LICENSE)). Premium and hosted offerings
("Caliper Cloud", commercial editions) may be built on top under separate terms;
the CLA is what makes that dual distribution possible. See
[`LICENSING.md`](LICENSING.md) for the full picture.

Questions? Open an issue or start a discussion. Thank you for helping build Caliper.
