# Contributing to Hyperion

Thanks for looking. This file is the short, practical version: what to run, what gets reviewed
hardest, and what will get a change sent back. The thinking behind the project is in
[`CLAUDE.md`](CLAUDE.md); the governance model is
[docs/40](docs/40-open-source-governance.md).

## Getting set up

```sh
cargo build --workspace
cargo test --workspace
```

On **Linux**, one extra target first — several tests spawn a real, statically-linked binary into a
real Landlock/seccomp sandbox, where a dynamically-linked one can't start:

```sh
rustup target add x86_64-unknown-linux-musl
```

Without it those tests fail with cargo's ``can't find crate for `core` ``, which doesn't name the
cause. On macOS they're `#[cfg(target_os = "linux")]`-gated and don't run.

Building a bootable image needs QEMU and Buildroot host dependencies; see
[boot/README.md](boot/README.md).

## Before you open a pull request

These are exactly what [CI](.github/workflows/ci.yml) runs, so running them locally saves a round
trip:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo doc --workspace --no-deps        # broken intra-doc links fail the build
scripts/check-claims.py                # see "Claims" below
```

The pull request template asks six questions. They come from `CLAUDE.md` and they're the actual
review criteria, not a formality — a one-line answer that's true beats a paragraph that isn't.

## What gets reviewed hardest

**The six design invariants** ([docs/02 §4](docs/02-core-architecture.md#4-design-invariants)) bind
every subsystem: no silent authority, everything undoable or versioned, local-first by default,
every autonomous action explainable, degrade rather than fail closed, accessibility is not a mode.
A change that touches what one of these *means in practice* is not an ordinary pull request — it
needs an RFC, a 30-day comment period, and a supermajority TSC vote. See
[docs/40](docs/40-open-source-governance.md). Most changes don't come near this; the ones that do
are usually surprised to learn it, so say so in your PR if you think yours might.

**Tests that would fail if the change were wrong.** "Added a test" isn't the bar. The bar is that
removing your fix makes the test fail. If you can, check: break it on purpose, watch the test go
red, put it back. Several tests in this repo were written that way and say so in their comments.

**Never assert on wall-clock budgets.** "This must finish in under 350ms" makes the *result* depend
on machine load, and every test in this repo that did it has since failed on a loaded CI runner
while behaving perfectly. Poll for the outcome under a generous deadline, or assert on the property
directly — e.g. that two operations' time spans overlap, which is disjoint under serialization
however fast the machine is.

**Honest scope.** This codebase says what it hasn't done. Crate docs carry "Deliberately deferred"
lists; completion notes name what's still missing. If your change closes half of something, say
which half — a named gap is worth more than an unnamed one, and far more than a claim that
overreaches.

## Claims

[`claims.toml`](claims.toml) pairs the guarantees this project makes — enforcement boundaries,
durability, consent gates, cryptographic properties, erasure promises — with the exact test that
fails if each stops being true. `scripts/check-claims.py` runs in CI.

If you change behaviour a claim depends on, update the claim in the same pull request. If you find
yourself wanting to *delete* one, that's a conversation, not a cleanup.

If you add a guarantee someone could be harmed by believing wrongly, add a claim for it.

## Errors and messages

Two audiences, and the rule is the same for both: never make someone guess.

For users, `NullPointerException` is not an error message. "I couldn't finish importing your photos
because one appears to be corrupted" is. Keep the technical detail — put it in the log, not the
sentence the person reads.

For developers, the same applies to a failure they'll hit. A test that dies with
``can't find crate for `core` `` should say the target isn't installed and name the command.

## Commits

One logical change per commit. The message should say what was wrong and why the fix is right —
the diff already says what changed. If a bug had a non-obvious cause, that cause belongs in the
message, because it's the thing the next person needs and the only place it survives.

## Reporting a security issue

Please don't open a public issue for a vulnerability. Report it privately through
[GitHub's security advisories](https://github.com/JGalego/HyperionOS/security/advisories/new),
which reaches the maintainers without disclosing it first.

Hyperion is experimental and has never been audited. It's not ready to hold anything you'd mind
losing or leaking — but reports are genuinely welcome, and the capability, sandboxing, consent, and
erasure paths are the ones worth pointing at.
