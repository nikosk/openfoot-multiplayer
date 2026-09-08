# OpenFoot Multiplayer

A standalone, headless multiplayer football-management simulator in Rust.

Status: foundation stage, not yet a playable multiplayer game. The initial import
is OpenFoot's match engine and its tests. See [the plan](plans/0001-foundation.md).

The simulator will support human, scripted and external automated managers through
the same rules and protocol. It has no dependency on a particular agent runtime
or evaluation product. Spectating will be API-first, not tied to a graphical client.

Run the imported engine tests with `cargo test --workspace --locked`.

## License and attribution

The repository contains the GNU GPL version 3 in [LICENSE](LICENSE). Imported
OpenFoot code retains its upstream GPLv3-or-later grant and attribution; see
[UPSTREAM.md](UPSTREAM.md). The precise license declaration for newly authored
implementation files will be recorded before adding those files; the license text
alone does not select an “or later” grant.
