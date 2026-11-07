# Contributing to Sculptured

Thanks for helping. Bug reports, ideas, docs fixes and code are all welcome.

## Before you start

- **Bugs and ideas:** open an issue with the templates. For anything large, open an issue first so we can agree on the approach before you write code.
- **Security problems:** do not open an issue; see [SECURITY.md](SECURITY.md).
- Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Set up

You need Rust 1.95 or newer, Node.js 24, and Git. Claude Code or Codex is only needed to run real agents; the tests never call them.

```bash
git clone https://github.com/Sculptured/Sculptured.git
cd Sculptured
npm --prefix web ci && npm --prefix web run build
cargo run -p sculptured -- serve          # http://127.0.0.1:4410
```

How the code fits together is in [docs/internals/](docs/internals/README.md); start with the overview.

## Make a change

1. Branch from `main`.
2. Write a test that fails without your change, then make it pass.
3. Run the checks:
   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets
   npm --prefix web test
   npm --prefix web run lint
   ```
   Lint currently reports older size-rule problems (long functions and files) that are being paid down; a change must not add new ones.
4. If you changed the UI, look at it in the running app at desktop and phone widths, and say in the pull request what you checked.
5. Open a pull request and fill in the template.

## Ground rules

These keep the product honest; reviewers will hold changes to them. The full list is in [AGENTS.md](AGENTS.md).

- **Every agent runs a real model, and routine tests call none.** Logic that needs no agent is a plain test. Behaviour that needs an agent is a live test on the real Claude Code CLI with haiku: named `live_…`, `#[ignore]`d, run with `cargo test -p <crate> --test <file> -- --ignored live_` (needs `claude` signed in; costs credits). There is no scripted or fake agent; never add one.
- **Never claim more than we know.** Keep planned, in progress, reported complete, verified and blocked distinct, and label where information came from. Unknown stays unknown.
- **Plain words on the main path.** Advanced concepts stay under Advanced.
- **Every drag has a keyboard path**, status is never colour alone, and the phone layout shows the same evidence.
- **TypeScript API types are generated** from Rust into `web/src/generated/`; regenerate with `cargo test --workspace export_bindings`, never edit them by hand.
- **SQL lives in `crates/controld/src/store/`.**

## Commit messages

Write the subject as what the change does for a user, in the imperative ("Keep the issue marker on the card's name row"), and explain why in the body.

## License

By contributing you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).
