## What this changes

<!-- What a user notices, and why. Link the issue: Fixes #123 -->

## How it was checked

- [ ] A test that fails without this change
- [ ] `cargo test --workspace`, `npm --prefix web test` and lint pass
- [ ] UI changes looked at in the running app (desktop and phone width), or not applicable
- [ ] No real model calls or third-party requests in routine tests (anything needing an agent is an `#[ignore]`d `live_…` test)

<!-- Anything you could not check, say so here. -->
