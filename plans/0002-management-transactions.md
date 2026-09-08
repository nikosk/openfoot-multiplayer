# Management transaction foundation

Status: implemented foundation slice, with focused tests. This is a state/transaction slice,
not a football economy, multiplayer server or feature-parity release.

## Scope

Implement explicit fixed managers/clubs, a small player ownership registry,
seller-consented offers, non-blocking previews and atomic confirmation. Add
daily readiness/deadline enforcement and participant elimination. The core is
single-owner/synchronous: the host dispatches admitted commands in arrival order;
no world lock is held while a participant thinks. A network queue is not included.

Actor identity and time are trusted host inputs, not fields a client may select.
Internal state is private; public state excludes money and negotiations. A manager
sees its own club balance and offers involving its club, not other balances.

## Rules for this slice

- A buyer's offer authorizes the stated fee, not a reservation. The owning seller
  reviews and confirms acceptance. Players have no new willingness formula here;
  integrating actual player/contract/registration rules remains required.
  Buyer-side preview/confirmation before offer submission is not implemented yet;
  this demonstrates seller acceptance only, not the complete review workflow.
- Confirmation dependencies include both clubs' balances and financial revisions,
  player ownership/revision and offer terms/status. Intervening financial changes
  invalidate a preview even if affordability still holds; unrelated offers do not.
- A stale confirmation returns a fresh preview without executing the deal. New
  confirmation is required. Missing ownership, insufficient funds or unavailable
  offer returns an error, never an executable preview.
- Every command has a manager-scoped request ID and day. Identical retries return
  the original receipt. Reusing an ID with different content is rejected. Former
  managers cannot retrieve old private receipts after elimination.
- Ready blocks new offers, but permits reviewing/confirming/rejecting existing
  incoming offers while open. All-ready or inclusive deadline closes the window.
  Ready does not wait for an outstanding preview. No next-day football simulation
  is implemented yet; closure is observable for a future authoritative day runner.
  Observed closure is latched: earlier timestamps cannot reopen a closed window.
- Elimination is host-only. It revokes offers involving the fired participant's
  club in this prototype to prevent stale commitments. Full replacement policy and
  handling of committed deferred registrations are not implemented or selected.

## Verification

Focused unit/integration tests: invalid setup and ownership, private projections,
seller consent, ready/deadline, failed transaction invariants, stale financial
preview, unrelated mutation, competing offers, idempotency and firing. Run the
imported engine tests too; add a model-free scripted example using public APIs.

## Remaining release work

Verification on 2026-09-08: `cargo test --workspace --locked --offline` passed
160 tests (143 unchanged engine tests, 5 window tests, 12 transaction tests).
`cargo check --workspace --all-targets --locked --offline`,
`cargo fmt -p management -- --check`, and the scripted transfer example passed.
Independent review examined privacy, atomicity and stale-confirmation boundaries.
No model-backed calls or football career runs were performed.

Host identity/time must be authenticated and authoritative. Receipt/offer/preview
IDs expose aggregate command sequence, although not private payloads; activity-hiding
identifiers and resource/retention limits remain protocol work. No durable receipt
or save/load guarantee is claimed by the current in-memory implementation.

World import/clone equality, full contracts/loans/scouting/training/finances,
native bot policies, authentication/transport, scheduled football progression,
durable validated save/load, event subscriptions and narrator integration remain.
This slice must not be presented as completing those requirements. Newly authored
implementation license variant still needs an explicit declaration; the committed
GPLv3 text is preserved without changing imported upstream grants.
