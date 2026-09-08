# Persistent pre-match tactics

Status: implemented slice; not full squad/tactical parity.

## Contract

Expose the pinned engine's six play styles and nine phase-tactic dials as an
explicit, typed club MatchPlan. A SetMatchPlan command atomically replaces the
whole plan through the existing authenticated FIFO/readiness/idempotency path.
Managers can retrieve only their own saved plan; public state excludes it.
The saved plan persists across days and seeds each delegated match for its owner.
The existing automatic match AI may adapt during a match; it must not overwrite
the saved plan for the next fixture. Default behavior stays Balanced/default dials.

Use existing engine enums, without changing imported engine files. An owned
Eq-compatible record supports request identity checks, with an explicit conversion
to engine TacticsConfig (which lacks Eq). No new tactical formulas.

Source inspected: pinned engine types.rs, shared.rs tactical modifiers,
live_match/simulation.rs and ai.rs; upstream commands/squad.rs set_tactics_phase.
Unlike the old string command, invalid enum values fail deserialization rather
than silently selecting a fallback.

Formation remains 4-4-2: detailed formation-slot mapping, player-role commands,
set pieces and delegation-profile configuration require further integration.
No live or halftime intervention; no public disclosure of unpublished tactics.

## Verification

Exact field conversion/defaults; own-club read/write; ready/late/unknown caller
denial; request replay and changed-payload rejection; serialization rejects invalid
dials; unrelated tactics leave transfer previews valid; both clubs' plans reach
the actual delegated engine (compare against directly constructed seeded matches);
saved plans survive resolution. Full workspace tests and all-targets check.

Completed: 199 workspace tests pass offline with the lockfile, including exact
seeded engine-report comparison and independent sensitivity to each club's plan.
All-targets compilation, management formatting, and both model-free examples pass.
Existing tests are retained and imported engine files are unchanged. No model run.
