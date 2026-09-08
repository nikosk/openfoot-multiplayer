# Contracts, board employment and continuing seasons

Status: implemented for the current single-league prototype. No paid competition
was launched by this change.

## Contract

Implement the missing player-contract lifecycle, private board evaluation and
irreversible dismissal, and an actual season transition for the current domestic
league. Reuse the pinned upstream rules rather than inventing replacement scoring.
Do not claim cups, promotion/relegation, youth generation or unrelated training
parity merely because this single-league lifecycle works.

Contract records need dates, wages and player negotiation inputs from the source
world. Support retrieval, renewal terms/counters, letting contracts expire,
termination preview/confirmation, free-agent signing, real expiry and weekly wage
charges. Consequential terms must be reviewed against current ownership, finances,
contract revisions and date before atomic commitment. Preserve request receipts,
readiness restrictions and cross-manager privacy. Do not silently extend expired
contracts or reset depleted funds to avoid a failed club.

Board records are per manager, initialized equally for cloned clubs. Preserve
upstream reputation-scaled league/win/goal/financial objectives, match satisfaction
changes and warning/firing thresholds. Warnings remain private; dismissal is public.
Fired participants cannot read private club state, submit late/replayed commands,
or regain control after rollover. The club receives a distinct bot manager under
the same rules. Preserve the dismissed participant's outcome separately.

Rollover requires a complete, valid league schedule and once-only settlement.
Archive results/standings and manager outcomes, apply supported upstream prizes
and objective evaluation, generate the next calendar with unique IDs, and preserve
ownership, player condition/fitness, balances, contracts, saved plans and dismissal.
Game day and contract dates must continue monotonically. Keep the one-season
experiment horizon distinct from the simulator's ability to continue seasons.
Private save/reload and journal replay must preserve these effects and receipts.

## Implementation sequence

1. Characterize pinned contract/board/rollover arithmetic and permission pressures.
2. Add explicit domain records and command/lifecycle integration; stage failed days
   and failed settlement atomically rather than partially applying wages or expiry.
3. Update source cloning, host bot/terminal handling and exposed tool schemas.
4. Verify focused boundary cases, dismissal/replacement, contract consequences and
   two-season state continuity with small unit/integration fixtures. No separate
   long scripted tournament or model call is needed for this work.

Any remaining unsupported rule must be named in the final report; adding a tool
name or a year counter alone is not completion.

## Delivered boundary

Contract negotiation, atomic previews/confirmation, free agents, expiry, wages,
board evaluation/dismissal/replacement, season archives/prizes/reputation/calendar,
private checkpoint restoration and host/client routing are integrated and tested.
Source salary naming is preserved, but Monday charges use the executable annual
unit. Daily closing-date effects occur before advancing the date. Recovery age
and morale derive from current contract records, not frozen import profiles.

The source importer preserves debt and real contract/calendar inputs and creates
equal new-manager board states for both cloned clubs. Prototype bots renew at
expected terms inside 180 days through the normal command path; this is an
explicit limited policy, not a claim of complete upstream bot behavior.

Remaining limits: paid transfers retain existing contracts rather than negotiating
player consent; loans/delegated assistant renewals, sponsorship/gate income,
injuries/suspensions, retirement/youth, training growth and promotion/cups remain
unsupported. Board runway is explicitly wage-only. Replacement bots start
immediately with satisfaction 50; all managers use the same external-manager
match satisfaction rule. Valid expiry can leave fewer than eleven, and the next
match then fails explicitly unless a manager recruits; no invented roster rescue.

Verification includes the full Rust workspace and all-target check, private HTTP
tests, importer/replay tests, two-season asset/receipt/firing continuity, checkpoint
roundtrips and exclusive file saves. The real 20-club/440-player cloned London
scenario initialized with zero simulated matches and zero model calls.
