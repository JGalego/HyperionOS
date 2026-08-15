<!--
CLAUDE.md's "Pull Request Expectations" already state what a Hyperion pull request should answer.
They only existed there, where nobody opening a PR was looking. This is the same list, at the
moment it applies.

Answer briefly. A one-line answer that is actually true beats a paragraph that isn't. Delete a
section only if it genuinely doesn't apply, and say so rather than leaving it blank.
-->

## What problem does this solve?

## Why is this solution correct?

## What alternatives were considered?

## Does this increase or reduce complexity?

## How was it tested?

<!--
Name the tests. "Added a test" is not an answer; "a test that fails if the guard is removed" is.
If the change can't be covered by a test, say why, so the gap is named rather than absent.
-->

## Does it improve the user experience?

<!--
Both users count: someone speaking a goal to Hyperion, and whoever next has to read this code.
"Neither, it's internal" is a fine answer.
-->

---

<!--
Before requesting review, these should pass locally -- the same things CI runs:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked

Linux also needs `rustup target add x86_64-unknown-linux-musl` for the sandboxed-binary tests;
see README.md's "Working on Hyperion".
-->
