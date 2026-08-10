# LogBrew CLI Contributor Guide

This checked-in root guide applies repo-wide. Keep exact behavior in source,
tests, scripts, and workflows.

## Ownership

- `src/parser.rs` and `src/parser/` own command grammar and aliases.
  `src/help.rs` owns user-facing help. Change them together when public usage
  changes.
- `src/render.rs`, `src/error.rs`, `src/main.rs`, and command modules own
  validated output, recovery, process exit, and stdout/stderr boundaries.
- `src/auth.rs` and `src/auth/` own credential selection, storage, refresh, and
  redaction. Keep authentication policy and authenticated request handling in
  that layer.
- [Cargo.toml](Cargo.toml), [tests](tests), [scripts](scripts),
  [.github/workflows](.github/workflows), and
  [dist-workspace.toml](dist-workspace.toml) own exact commands, flags, cases,
  checks, and release topology. Treat the nearest focused test as the behavior
  contract.

Preserve unrelated work and keep a change in the layer that owns it. Reuse
existing parsers, transports, renderers, auth helpers, and fixtures before
adding a new abstraction.

## Stable Contracts

- Preserve each command's tested exit, stdout, stderr, human, and JSON behavior.
  JSON stays valid and deterministic. Human output stays bounded,
  scan-friendly, and actionable.
- Fail closed on malformed, oversized, contradictory, redirected, or
  unrecognized input and responses. Never turn invalid data into partial
  success or reflect raw server errors.
- Never print or log credentials, authorization material, credential locations,
  or raw request payloads. Redact before rendering and cover hostile echo cases
  at the touched boundary.
- Keep credential types distinct. Route refresh, persistence, ownership, and
  permission behavior through the canonical auth layer.
- Keep source and public metadata free of secrets, customer data, local
  absolute paths, operational details, unpublished strategy, research notes,
  and local planning material.

## Evidence

Verification should match risk and blast radius. Start with the nearest focused
regression and add lint, formatting, confidentiality, diff review, or package
checks when those surfaces change. Trust boundaries such as auth, filesystem,
archive, transport, and privacy require adversarial fail-closed coverage and a
built-binary loopback when behavior crosses that boundary.

Use the owning scripts and complete local gate for shared dependencies,
packaging, workflows, or release changes. Do not substitute a broad green suite
for a missing focused regression, and do not run unrelated expensive checks
during focused iteration.

Treat source tests, workflow plans, published bytes, and installed execution as
separate release evidence. Bind release evidence to exact source, version,
workflow, and digest inputs. Preserve historical evidence, and change generated
release workflows through their pinned owning configuration.

## Code Review Rules

- Flag command grammar and help drift, changes to tested output or exit
  behavior, fail-open response handling, and any path that can render raw
  server data or credential material.
- Flag generated release state changed outside its owner and release claims
  that are not bound to the exact published and installed artifact.
- The safe path is to change the owning parser, renderer, auth, or release
  configuration; add the nearest adversarial regression; and run the checks
  for that boundary. Keep formatting and other mechanical policy in scripts or
  CI rather than expanding this guide.

## Continuous Simplification Gate

- Before each implementation step, record a reproducible handwritten source
  LOC baseline for its exact scope. Use the same command and scope afterward,
  then report the baseline, final count, and net change.
- Complete a step or run only when net maintained production and test source
  LOC decreases. Exclude generated, vendored, lock, cache, and build output.
- Offset additions by deleting more duplication, obsolete branches, wrappers,
  helpers, compatibility scaffolding, or parallel implementations. Prefer one
  canonical flow and fewer concepts over equivalent reorganization.
- Preserve required behavior, tests, type safety, coverage, compatibility,
  documentation, validation, security, observability, and error handling.
- Never manufacture reduction with minification, line packing, formatting
  tricks, generated-code moves, or deletion of valuable tests or docs. If safe
  reduction is not possible, leave the step incomplete and report the blocker.

## Public History

Public files, commit and pull-request text, fixtures, logs, and generated
artifacts are permanent. Use generic product-facing metadata. Do not tag,
publish, dispatch release workflows, or alter registry state unless the task
explicitly owns that action.

Keep this guide concise and current. Add durable rules after repeated review
feedback; keep one-off context out of the repository. Do not add a duplicate
`CLAUDE.md` instruction surface.
