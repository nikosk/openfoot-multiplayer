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

`crates/management/src/physical.rs` adapts `ofm_core/src/player_wear.rs` from the
same revision to engine player records. The wear/sharpness formulas are preserved;
club injury rolls are not added. Physical-effects RNG stream selection and
persistence orchestration are new and not claimed equivalent to upstream runs.

`crates/management/src/selection.rs` derives a grouped-position fallback from
`ofm_core/src/live_match_manager/team_builder.rs`. It preserves all available
preferred starters rather than applying the original low-survivor rebuild rule,
and uses stable ID tie-breaking. Detailed formation-slot mapping is not ported.
Both files retain GPLv3-or-later upstream attribution and document deviations.
