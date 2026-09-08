# Gameplay parity is a release prerequisite

Status: source inventory implemented and actual-world rollover integration-checked;
final workspace/HTTP and rebuilt archival-scenario checks pending before launch.
This supersedes any suggestion that accepting
the current prototype's limits is sufficient for the first external-agent contest.

The current executable has passed the source lifecycle probe, not final release checks.
Do not launch that contest until the gameplay inventory has been implemented and
verified. Passing the existing tests does not certify missing systems.

## Meaning of parity

Preserve the selected pinned OpenFoot baseline's management capabilities and
causal rules, including delayed effects, ordinary failure paths and persistence.
Expose them through multiplayer-scoped observations and actions. A similarly
named tool or an approximate substitute is not evidence of equivalent behavior.

Previously agreed multiplayer differences remain: fixed clubs, terminal manager
dismissal, shared daily deadlines, response-only readiness and atomic FIFO
review/confirmation. Bots may be reimplemented under shared permissions; exact
single-player decisions and transaction privileges need not be reproduced. This
does not authorize a reduced bot management repertoire or arbitrary balance changes.
Matches remain delegated; live manager intervention is deferred. Desktop rendering
and editor UI are not requirements. Any further gameplay omission requires an
explicit user scope decision; inactivity in one scenario is not permission to
delete the mechanic.

## Blocking work inventory

1. Complete team/individual training, intensity/schedules, development, persistent
   injuries/suspensions and their recovery/selection consequences.
2. Complete formations, roles and set pieces; preserve delegated match behavior.
3. Complete recipient-scoped inbox, conversations, morale/trust/grievances,
   commitments and delayed consequences.
4. Complete scouting, staff markets/delegation and facility costs/effects.
5. Complete transfer negotiation/player consent, listing/counters, loans, wage
   shares/options/returns, registration windows and shared-market bot policies.
6. Complete finance income/support/sponsorship/marketing and ledger processing;
   remove the prototype's wage-only economy as a substitute for the baseline.
7. Complete competition/calendar coverage: friendlies, cups, promotion/relegation,
   qualification and active/dormant lifecycle; inventory national football rather
   than silently declaring it irrelevant.
8. Complete aging/retirement/youth, awards/statistics/history, and lossless import
   and cross-season persistence for the above systems.
9. Replace simplified bot policy with native participants covering the supported
   management systems. Review provisional board/hiring policy differences against
   the agreed multiplayer exceptions rather than treating documentation as approval.

Existing contracts, board and rollover slices also require integration review
against these dependencies. They are not exempt because their unit tests pass.

## Evidence required before launch

For each operation, record the selected source/patch reference, its observation
and action, permissions, immediate/delayed effects, failure behavior, persistence
tests and any approved deviation. Compare extracted mechanics against equivalent
source inputs where practical. Validate bot/external combinations, clone equality,
private/public boundaries, once-only daily processing and continuing seasons.

Only a completed inventory and passing integration evidence close this prerequisite.
No deadline or playable subset substitutes for it. Model-free development tests
are allowed; the external-agent contest remains blocked on parity.

## Active implementation sequence

Begin with complete training/development and availability, plus formation/role/
set-piece management, because they establish player-state inputs consumed by
scouting, staff and bot decisions. Preserve explicit ownership of attributes,
training metadata and availability; do not create per-manager worlds or replay
the old global day loop for every manager. New commands use existing receipts,
readiness, authorization and once-only staged daily processing.

Source inspection may establish that a listed feature is absent from the baseline
(for example, a persistent ban must not be invented merely because match cards
exist). Record the actual source behavior and evidence instead of manufacturing
extra football rules to check an inventory box.

## Evidence refresh — 2026-09-08

`UPSTREAM.md` now has one source/module/evidence row per implemented subsystem:
squads/matches, training/availability, conversations, contracts/boards,
scouting/youth, staff/facilities, markets/loans, finances, competitions,
aging, team history/awards, match statistics/original archives, news,
national/World Cup football and multiplayer/native participant boundaries.
It replaces stale initial-prototype statements without declaring the gate closed.

Concrete model-free checks run during this slice:

- Final workspace Rust tests pass (one explicitly manual integration test ignored),
  HTTP host tests pass 10/10, and journal replay passes after the integrated edits.
- `news_runtime` tests exercise actual closed-day hooks, both managers' private
  reminders exactly three days ahead, deterministic cloning, once-only delivery,
  imported news/protected clubs and checkpoint restoration.
- `youth::national_generation_uses_senior_slot_and_clears_only_contract_ownership`
  compares all 22 source slots with explicit identical RNG and generated fields.
- `team_history` and `player_history` tests cover completed competition/season
  identity, unreset foreign midseason statistics and persistent original dismissal.
- Actual export probe completed 71 model-free days, World Cup progression and
  checkpoint loading. The source contains 9,823 projected live players; the
  original source/replaced identities are separately archived, never additional
  causal players or managers.
- Rebuilding the actual export in memory resolves all 27 previously dangling
  historical award references through 44 original players, two original teams
  and two original managers. Global WorldHistory/statistics stay exactly equal
  to the source data. Two synthetic award records and archive privacy/immutability/
  checkpoint tests cover the regression.
- Configured tied tables use source stable standings order and one champion;
  a source-order/checkpoint regression passes. Legacy explicit fixtures retain
  their documented ID ordering.
- External client checks pass: nine wait-loop tests (including private same-day
  transfer notices), four schema checks, syntax and no-inference `--help` checks.
  The existing WorldGym OpenFoot experiment remains unchanged: `npm test`
  passes 13 test files and `npm run typecheck` passes.
- Actual host fairness probe reported initial listening at 11.6 seconds followed
  by a full 120-second external decision window; external Ready responses took
  365 ms and 80 ms while native bots were active. Narration remains asynchronous.
- Exact Pi model/low-reasoning/credential-presence preflight succeeded without
  inference. `--allow-model-run` is only execution opt-in, not parity acceptance.

The corrected full-world rollover probe has passed. Its days 321–324 exposed
overlapping Argentine Apertura/Clausura phases
being incorrectly passed to the source's disjoint promotion-pyramid helper.
The explicit precondition guard now preserves overlapping phase memberships;
disjoint pyramids still execute the unchanged source swaps, and financial/prize
formulas are untouched. The resumed actual-world probe reached day 324 with 799
domestic matches, season 2026 settled on day 321 (April 17), source cup/promotion/
re-registration and the 2027 calendar, plus 485 national friendlies scheduled.
The final checkpoint loaded in 8.5 seconds:
`/tmp/fullworld-rollover-complete-day324-1788899995803.json`.

The source inventory is implemented and integration-checked. Final workspace/HTTP
tests and the rebuilt archival-scenario API smoke pass (440 clubs, 9,823 players,
national fixtures, historical identities and private observation isolation).
The requested paid contest may now launch. Explicit RNG streams and the authorized multiplayer
deadline/authority/consent/dismissal differences mean this is not bit-identical
desktop-app replay. No additional gameplay omission is declared or authorized.
Imported engine/domain trees remain unchanged; no paid contest has run as part
of these checks.
