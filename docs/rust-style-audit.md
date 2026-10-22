# Rust style audit

The Cargo workspace under `crates/` was reviewed against the
[Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html)
on 2026-10-21. The review covers production code, fixtures, and Rust tests. It
preserves the project's requirements for focused modules, shared implementations,
and small functions whose boundaries express a meaningful responsibility.

## Guide coverage

| Guidance | Application in this workspace |
| --- | --- |
| Formatting and naming | Rustfmt uses edition 2024 and a 100-column limit. Imports are grouped by module. Names follow Rust conventions. Fallible constructors use `try_new`. |
| Modules and imports | Implementations live in leaf modules, with existing module paths retained through facade re-exports. The small projection dispatcher and shared clock helper remain permitted glue. Cross-module free functions are qualified. Glob imports are limited to inline test `super::*` and facade re-exports. |
| Function design | Helpers separate responsibilities and share repeated work. Long iterator expressions are named before loops. A coherent function is not split solely to meet the existing 30-line Clippy threshold. |
| Ownership and parameters | Read-only string and optional string parameters borrow `str`; mutable containers remain where callers need structural mutation. Local iterator closures and collections of references retain their required element types. Owned launch and message values transfer responsibility to their consumer. |
| Errors and panics | Recoverable failures use `Result` or `Option`. Production invariant checks explain their assumptions with `expect`. Invalid workflow state returns an error. Corrupt persisted journal JSON returns a database conversion error. Poisoned mutex state is never recovered and reused. |
| Types and traits | Existing derives and standard conversion traits express value semantics. Required launch settings do not receive artificial defaults. Pure return values and consuming builders use `#[must_use]` where discarding the result would lose the operation. |
| Public contracts and documentation | Driver/session responsibilities and caller-triggered panic conditions are documented. The serialized protocols remain closed, coordinated application schemas; adding `non_exhaustive` would prevent exhaustive handling inside this application. |
| Unsafe and concurrency | Unsafe blocks are limited to operating-system operations and document safety assumptions. Process-group signaling and session detachment require facilities absent from safe `std` APIs. Shared ownership supports concurrent runtime tasks and shared stores. |
| Panic handling | PDF extraction keeps a documented defense-in-depth boundary around a third-party parser. Its normal failure path is `Result`, and neither correctness nor memory safety relies on unwinding. |
| Stable language and dependencies | There are no unstable Rust language feature attributes. Cargo dependencies and the established Tokio runtime remain the application's build and concurrency model. |

## Scope decisions

Chromium/Fuchsia build, test-framework, and runtime requirements apply to those
environments, not this Cargo application. This workspace uses Cargo tests and
Tokio. Its crates are unpublished and maintained together; public visibility
between these crates does not by itself establish a separately versioned API for
another team.

The guide allows judgment around module size, function size, iterator parameters,
and documentation that would merely repeat a name. The 30-line Clippy setting is
advisory, as requested. It remains visible rather than being raised or suppressed.
Tests may panic on failed setup and assertions; fixtures intentionally fail when
their test protocol cannot be produced.

## Validation

The refactor is checked with workspace formatting, all-target compilation,
Clippy, and the complete offline Cargo test suite. A string-literal comparison
against the pre-audit source preserved every existing non-documentation string
except the journal panic message removed by the recoverable error path. This
also checks that prompt text, protocol strings, and command arguments survived
the module and import changes. Generated TypeScript is not edited by hand.

The journal regression test `corrupt_event_is_returned_as_a_database_error` in
`crates/hostd/tests/journal.rs` verifies the changed error behavior. Socket and
process integration tests additionally require an environment that permits
their listeners, Unix sockets, and temporary process homes.

The full offline run completed with 960 tests passing, 338 failing, and one
ignored. The failing set matches the pre-audit run: 315 denied operations, ten
unavailable sockets, six missing server-listening messages, and seven read-only
home-directory failures. The new journal test passes. Formatting and all-target
compilation pass; Clippy reports only the advisory function-length warnings.
