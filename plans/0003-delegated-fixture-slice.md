# Delegated fixture and squad slice

Status: implemented prototype slice. This connects existing transaction state to
the imported engine, not a complete season/economy implementation.

Follow-up: [plan 0004](0004-squad-repair-and-wear.md) supersedes this slice's missing-XI
failure and nonpersistent-wear limitations. The historical notes below describe
the original slice; fewer than11 eligible players still needs roster-safety work.

- Add manager-owned starting-XI commands using the same authorization, readiness,
  day and idempotency boundary as transfers.
- Store engine player attributes by immutable player ID. Club ownership remains
  authoritative in management state; match construction must consult it afresh.
- Import explicit fixture inputs with per-fixture seeds. No automatic calendar or
  benchmark world generation is claimed. Both sides use the same delegated live
  engine path and the same default automatic match-decision profile.
  This slice uses balanced 4-4-2, default tactics and up to twelve remaining owned
  players in registry-ID order as bench. Tactical configuration and position-aware
  bench policy still need their own management commands/parity integration.
- Advance a closed day once, stage all match reports before committing results,
  retain standings and offers, clear stale previews/readiness for the next day.
  A retry with the old expected day cannot duplicate results. A day cannot resolve
  a fixture unless both selected XIs still belong to their clubs.
- Reserve goalkeeper/position-aware automatic roster repair for subsequent domain
  integration. This slice rejects an invalidated XI instead of silently overriding
  an external manager. The full release must handle injuries, transfers and absent
  managers without permanently stalling a league; this is a documented blocker.

## Verify

Verification on 2026-09-08: full workspace tests, all-target compilation and
management formatting checks. The scripted fixture example completed two seeded
fixtures and produced standings. Independent review checked privacy, duplicate-day
guards, live ownership lookup and staged result commitment. No model calls.

Deterministic reports from seeded explicit fixtures; same lineup path for both
clubs; authorization/readiness protection; transfers change actual squad ownership;
invalid XI causes no partial day/result changes; deadline advances despite an
unready manager; retry cannot replay a day; pending offers carry over while previews
and readiness expire. Run all existing tests and a scripted two-club match example.

Training, wear carried across days, injuries/suspensions, contracts, native daily
bot planning, automatic scheduling, saves and public subscriptions remain pending.
Standings use points, goal difference, goals scored then ID for stable display;
the last key is not an approved competition tie-break or model-comparison rule.
The match engine's own events and condition calculations still execute; do not
claim durable career consequences not yet applied by this adapter.
