# Squad repair and persistent physical wear

Status: implemented slice, not full training/medical parity.

## Source findings and decisions

Pinned OpenFoot `ofm_core/src/player_wear.rs::apply_match_wear` reduces condition
by minutes/stamina and probabilistically increases fitness after 60 minutes.
`turn/post_match.rs::deplete_match_stamina` calls that function for participating
club players. The separate `roll_match_injury` is used by national football, not
that club post-match path. Do not add a new club injury roll or infer injury duration
from a match event. Injury persistence requires a separate characterized mapping.

Adapt the wear formula to engine player records, attribute it, and apply it once
to report minutes using a deterministic physical-effects RNG. This is a new stream
derived from the explicit fixture seed, not exact original full-game RNG parity.
Sort player IDs before consuming it, so report HashMap order cannot affect outcomes.
Stage physical changes with match reports; no partial updates if a later fixture
fails. Do not also copy engine live condition into persistent state (double fatigue).

Repair missing/departed player selections without replacing valid selected players
for higher ratings. With no selection, choose a deterministic grouped-position
4-4-2 using existing player information; no external model required. Bench up to12.
This is an explicit initial fallback, not a complete bot planning policy or exact
upstream detailed-position slot mapping. Fewer than11 available players still fails;
real emergency roster/eligibility rules remain a blocker before autonomous careers.

## Checks

Verified 2026-09-08: 180 workspace tests passed with locked offline dependencies;
all-target compilation, management formatting and scripted fixtures passed. An old
failure-message assertion was updated from missing-XI wording to insufficient eligible
players, retaining the same no-partial-day invariant. No model-backed runs.

Zero-minute players unchanged; stamina100/90minutes loses24 condition; fitness does
not decrease; deterministic effects independent of map insertion order; selection
preserves still-owned starters and fills vacancy when reserves exist; transferred
player cannot play for old club; unused reserves not charged match wear; staged day
failure changes neither physical state nor results. Re-run all existing tests.

Rest/training recovery needs age, morale, staff and facility data not yet present
in this slice. Do not silently substitute a constant recovery bonus. Long-term
fitness/condition balance, injuries, suspensions and roster safety remain unfinished.
