# Upstream provenance

Source: https://github.com/openfootmanager/openfootmanager

Revision: `64677fee9047a1182005d666bafa5dbc025dca5c`.

Imported path: `src-tauri/crates/engine/` → `crates/engine/`, including tests and
Cargo manifest, unchanged. No career-specific or benchmark runtime patches are
included in this initial engine import. No desktop application or world dataset
is imported.

OpenFoot README credits Copyright (C) 2020–2026 Pedrenrique G. Guimarães and grants
GNU GPL version 3 or, at the recipient's option, any later version. Preserve
upstream notices when extending imported files. The full GPLv3 text is in LICENSE.

The engine's original dependencies are retained. The root Cargo.lock records the
resolved dependency versions for this standalone workspace, not a claim of full
application reproducibility or equivalence to a previously compiled game binary.

## Adapted management helpers

The gameplay extraction also retains previously reviewed career-runtime fixes
from `openfoot-career-runtime-v1.patch` (SHA-256
`dcffff321af5708c066139f3e25ce628797e2dd74dec3f04dbb4c455e4e10798`).
The source artifact is in nikosk/worldgym, `experiments/openfoot/patches/`.
These are part of the selected gameplay baseline, not new balance proposals:
annual-wage severance conversion, effective capped training/scouting facilities,
and the player-event and post-match corrections. Desktop/MCP transport changes
are not copied into the simulator. Engine/domain imports remain unmodified.
Each adapted subsystem must identify and test the applicable patch behavior;
the baseline selection is not a claim all of it is already integrated.

`src-tauri/crates/domain/` is now also imported unchanged as `crates/domain/`,
including its Cargo manifest and embedded tests. It provides the complete source
message, player, staff, team and competition records for the parity work; it has
no desktop or external agent-runtime dependencies. A recursive comparison against
the pinned checkout confirms byte-for-byte equality.

Training, availability and squad-plan modules adapt the same pinned core's
`training.rs`, `player_rating.rs`, `player_wear.rs`, `random_events/mod.rs` and
`live_match_manager/team_builder.rs`. Their source rules and multiplayer boundaries
are documented in each module. Seeded RNG is supplied explicitly; no upstream
global selected-manager state or thread RNG is used as multiplayer authority.

`crates/management/src/physical.rs` adapts `ofm_core/src/player_wear.rs` from the
same revision to engine player records. The wear/sharpness formulas are preserved;
club injury rolls are not added. Physical-effects RNG stream selection and
persistence orchestration are new and not claimed equivalent to upstream runs.

`crates/management/src/selection.rs` derives a grouped-position fallback from
`ofm_core/src/live_match_manager/team_builder.rs`. It preserves all available
preferred starters rather than applying the original low-survivor rebuild rule,
and uses stable ID tie-breaking. It remains the optional legacy fallback;
`squad_plan.rs` now ports detailed formation slots, role maps, set pieces,
footedness/alternate-position penalties and source bench selection for configured
worlds. Both clubs' configured plans feed the delegated engine.
Both files retain GPLv3-or-later upstream attribution and document deviations.

`crates/management/src/recovery.rs` adapts the uninjured Rest and Recovery-focus
branches and factor functions from `ofm_core/src/training.rs` at the same revision.
The host supplies explicit age/morale and physio/medical data; full player/staff
domain integration is supplied by the newer `training.rs` and `personnel.rs`
paths. Input validation and saturating condition addition remain legacy boundary
behavior; complete configured training uses the selected source rules.

## Integrated extraction evidence (2026-09-08)

All paths below are in `crates/management/src/` unless qualified. These are
implementation/test evidence, not a declaration that the parity release gate is
closed. The source references are relative to upstream `ofm_core/src/`.

| Surface | Selected source and implementation | Executable evidence |
| --- | --- | --- |
| Squad and delegated matches | `live_match_manager/team_builder.rs` and engine types → `squad_plan.rs`, `matches.rs`, `football.rs`; `SetLineup`, `SetSquadPlan`, `SetMatchPlan` | Formation slots, positional/footedness fit, roles/set pieces and both-club delegated wiring; imported engine remains unchanged |
| Training and availability | `training.rs`, `player_rating.rs`, `player_wear.rs`, random events and approved career patch → `training*.rs`, `availability.rs`, `physical.rs` | Team/group/individual development, source injuries/recovery and saved state; configured facilities affect development; no invented card suspension rule |
| Conversations and inbox | source message/conversation/turn rules → `inbox.rs`, `social.rs`, `social_daily.rs`, `contract_social.rs`; `Social` commands | Recipient-scoped decisions, commitments/delays, source morale/trust effects, retained resolved/deleted-message receipts and checkpoint tests |
| Contracts and board lifecycle | source contracts, `delegated_renewals.rs`, board/end-season rules → `contracts.rs`, `career.rs`, `delegated_contracts.rs`, `board.rs`, `football.rs` | Review/confirm, actual Monday payroll/closing-date expiry, assistant renewals, original dismissal permanence and source seven-day hiring lifecycle |
| Scouting and actual youth | `scouting.rs`, generator definitions/generation/nations and embedded default names → `scouting.rs`, `youth.rs`, `youth_source/`; explicit seeded RNG/caller IDs replace ambient RNG/UUIDs | Independent private reports, source delay/uncertainty, real pool generation/ranking, sign/shortlist/discard receipts and closed-day signing/checkpoint tests |
| Staff and facilities | source staff/club commands, regeneration and approved career patch → `staff.rs`, `facilities.rs`, `personnel.rs` | Competing hires, source wages/coaches/physio sync, 30-day staff refresh, training cap 6/scouting cap 3, medical behavior and effective scouting delay tests |
| Shared transfers and loans | source `transfers/` rules → `market.rs`, `market_daily.rs`, `market_rules.rs`; `Market` commands and private market view | Listing/bids/counters, reviewed consent, pending registration, loan wage shares/options/development/returns and lifecycle persistence tests; readiness permits supported responses only |
| Finances and commercial actions | `finances.rs` and sponsor/club commands → `economy.rs`, `economy_runtime.rs`, `finances.rs`; `Economy` commands | Source wages, sponsor/attendance income, marketing/support, ledger/cooldowns, financial board pressure and lifecycle tests; no wage-only substitute in configured worlds |
| Club competition lifecycle | source competition scheduling/groups/qualification, promotion and `end_of_season.rs` → `competition_*.rs`, `competitions.rs`, `promotion.rs`, `seasons.rs` | Active/dormant calendars, cups, overlapping dates, per-league rollover and midseason preservation; source-stable tied standings and single champion regression |
| Retirement and player history | source `end_of_season.rs` → `aging.rs`, `player_history.rs` | Exact decline/retirement arithmetic, retained retired wage metadata, live ownership removal, actual free-manager/scout generation and save/load tests |
| Team/manager history and awards | source end-season awards/history and `ai_hiring.rs` → `team_history.rs`, `season_awards.rs` | Per-competition/actual-season keys, untouched midseason foreign statistics, original-manager firing and seven-day caretaker-to-person hiring tests |
| Per-match statistics and original identities | `turn/post_match.rs`, domain stats/history records → `statistics.rs`, `team_history.rs`, `tools/import-world.mjs` | Stats captured before ownership changes; original historical IDs/records survive cloning in immutable noncausal archives. Actual export's 27 previously dangling award references all resolve; synthetic awards and private archive/save-load tests |
| Public news and private match messages | source `news.rs`, `news/match_report.rs`, `turn/news.rs`, `messages/match_messages.rs` → `news_*.rs` | Real daily symmetric three-day reminders, deterministic replay/checkpoint, outcome/fixture template tests; imported articles retained with explicit protected clubs |
| National teams and World Cup | `national_team.rs`, `world_cup.rs`, nations/generator source → `national.rs`, `national_matches.rs`, `national_world_cup.rs`, `national_nations.rs`, `youth.rs` | Actual national squads, international windows/carry-back, qualifying/tournament/hosts/rankings; all 22 generated senior slots checked; actual-world 71-day/World Cup checkpoint-load probe completed |
| Multiplayer transport and native participants | New adapter boundary, source-derived bot recommendations → `bot_manager.rs`, `bot_training.rs`, `bin/league.rs`, `tools/host.mjs`; external Pi tools | Owned receipts/observations, response-only Ready and private same-day transfer notice, fair external deadline under asynchronous bots, bounded retrospective beats and public historical/national/statistics queries; no privileged model tick/init |

The news runtime produces factual match reports, league/weekly/preseason digests,
transfer/loan/injury articles, appointments, awards and next-season previews.
It does not update statistics. Source manager career statistics update at season
settlement, not once per match; adding a per-match increment would double-count.
Public news filters articles dated after the current career date (including
future-dated imported articles), retaining raw imported state for later delivery.
Both date-only and RFC3339 source timestamps are accepted for that view.

The actual exported world has completed a model-free 71-day probe including
World Cup progression and checkpoint loading. Full-world rollover verification
then reached days 321–324 and exposed the source promotion
helper's disjoint-division precondition. Argentina's overlapping Apertura/Clausura
phases are not a disjoint promotion pyramid. `apply_disjoint_pyramid` now guards
that call and leaves those phase memberships intact; true disjoint pyramids still
use the unchanged source promotion/relegation function. This guard changes no
financial, prize or swap formula. Regression tests cover overlapping phases and
unchanged disjoint swaps. The corrected actual-world probe passed day 324:
799 domestic matches, season 2026 settled on day 321 (April 17), source cup/
promotion/re-registration and the 2027 calendar, plus 485 next-season national
friendlies. Its final checkpoint loaded in 8.5 seconds:
`/tmp/fullworld-rollover-complete-day324-1788899995803.json`.

The inventoried source systems are implemented and the actual-world season
lifecycle is integration-checked. Final workspace/HTTP checks and a rebuilt
archival-scenario API smoke remain pending before paid launch; see plan 0009.
This is not bit-identical replay of the original desktop application: explicit
seeded streams replace ambient RNG, while approved multiplayer differences
include shared deadlines/authority, FIFO consent and permanent original-manager
dismissal. No further gameplay omission is declared by this evidence.
Credential/configuration preflight confirmed the
requested Pi model IDs and low reasoning without inference; it is not a model
contest or a waiver. Engine and domain directory comparisons remain unchanged
against the pinned checkout.
