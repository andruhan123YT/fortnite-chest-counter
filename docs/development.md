# Development

How Fortnite Chest Counter counts, how to build it and how it is put together. For using
the app, see the [guide](guide.md).

## How it counts

Fortnite replays are Unreal Engine network recordings: the game state your client received
during the match, encrypted and compressed. The app decodes that stream and looks at the
chests in it.

- **Opened:** the chest's `bAlreadySearched` flag turns on and `SearchingPawn` names who
  opened it.
- **Broken:** when a chest is destroyed, by pickaxe, gunfire or vehicle, the server sets
  `SearchingPawn` to whoever destroyed it without marking it searched. A chest you already
  opened isn't counted again when you break it.
- **Treasures:** a prize grants the recording player a `GA_TreasureHandler` ability, so the
  replay shows exactly when it arrived and which nearby chest rolled it.
- **Opens the replay lost:** some recordings go minutes without chest updates (often the
  first minutes of a match) while the loot still records. A chest's worth of loot (3 to
  6 items, a weapon among them, thrown out of a container) landing at a player's feet,
  with no recorded open, break or ammo box there, counts as that player's open. The match
  timeline marks these "opened*". Items a player drops, on death or by hand, carry who
  dropped them and are never taken for a chest.

Apart from those opens found from loot, every count is a recorded event. As a sanity check over 46 matches: 97% of
the opens were followed by container loot landing next to the player within 2.5 s, and
94% of the breaks by the chest disappearing within 8 s.

Technical details are under [How it works](#how-it-works) below.

## Build from source

You need:

- **Windows 10 or 11** (64-bit).
- **Rust 1.92 or newer.** Install it from [rustup.rs](https://rustup.rs) and keep the
  default `x86_64-pc-windows-msvc` toolchain.
- **Visual Studio Build Tools** with the **Desktop development with C++** workload. Rust
  uses its linker, and its Windows SDK compiles the app's icon and version info. rustup
  offers to install it if it's missing.
- **Git**, to clone the repository (or download it as a zip from GitHub).

Then:

```sh
git clone https://github.com/andruhan123YT/fortnite-chest-counter.git
cd fortnite-chest-counter
cargo build --release
```

The first build downloads dependencies and takes a few minutes. The app ends up at
`target\release\fortnite-chest-counter.exe` and needs nothing else to run.

To run the tests:

```sh
cargo test --workspace
```

To build the same files the releases have (the exe and its checksum; needs Python 3):

```sh
python tools/package.py
```

They land in `dist\`. The same script keeps `crates/app/assets/licenses.json` (the list
the app shows under Settings > About) in step with `Cargo.lock`: if a dependency changed,
it rewrites the file and stops, so commit it and build again. `python tools/package.py
--licenses` only rewrites that file.

To release an update: add a section for it to `changelogs/en.md` and the same items to
every other file in `changelogs/` (tests fail if a language misses the version or an item),
set the new `version` in the root `Cargo.toml`, commit, then push a
tag with the same version (`git tag v1.0.1 && git push origin v1.0.1`). GitHub Actions
runs the tests, builds the exe and its `.sha256` and publishes them as a release
(`.github/workflows/release.yml`). Installed apps offer the update within 10 minutes. They
look for exactly those two file names, so keep them.

Not in this repository: build output (`target\`, `dist\`) and replay files (5 to 30 MB
each, and they contain other players' names). The island picture the map shows is
`crates/app/assets/map.png`, built into the exe.

Interface text stays in English in the code, wrapped in `tr("...")` or `trf("... {0}", &[..])`;
`crates/app/src/i18n/<language>.rs` holds each language's text, keyed by the English, with
its month and weekday names. A test reads the sources and fails on any wrapped text that a
language has no translation for (or that no code uses any more), so add every language
along with any new text. `plural(n, "match", "matches")` looks up `plural:match`, whose
translation lists the forms that language needs, split by `|` (Arabic five, Russian and
Polish three, French and Spanish two, Japanese one). Arabic, Japanese, Korean, Chinese and
Thai draw with Windows' own fonts (Segoe UI, Yu Gothic, Malgun Gothic, Microsoft YaHei /
JhengHei, Leelawadee UI), loaded only when needed.

Arabic goes through `crates/app/src/i18n/rtl.rs`. egui joins Arabic letters (HarfBuzz) but
has no bidirectional layout, so each string is put into display order up front
(`unicode-bidi`), every Arabic word turned back for egui to turn once more, and long text
broken into lines before that. Text bound for Windows, Discord or OBS goes back to plain
Arabic through `i18n::native`.

`tools/i18n_gen.py` built the tables from numbered translation files;
`python tools/i18n_gen.py keys` lists the English texts in order.

## How it works

The workspace has three crates:

| Crate | Purpose |
|---|---|
| `crates/fnreplay` | Reads the replay container (AES-256, Oodle), then walks the network stream the way Unreal's demo driver does: frames, packets, bunches, actor channels, net GUIDs, replicated property layouts, `ClassNetCache` fields and fast array deltas. Knows nothing about Fortnite. |
| `crates/chests` | The Fortnite schema (pawns, player states, chests, pickups, vehicles, abilities) and the analysis: opens, breaks, treasures, squads, drop odds. |
| `crates/app` | The egui desktop app, tray, overlay, folder watcher, replay archive, command line, online mode and the Epic profile script. |

`fnreplay` decodes only the properties the schema asks for and skips everything else
bit-exactly, since every property payload is length-prefixed.

Fortnite exports a replicated field once for a whole family of chest classes. A child
class such as `AlwaysSpawn_NormalChest_C` can replicate `bAlreadySearched` and
`SearchingPawn` long before (or without ever) exporting them under its own name, so
`fnreplay` resolves a missing handle through any class with the same schema group and the
same number of fields.

**Opens.** A chest update with `bAlreadySearched` set and a `SearchingPawn` is an open.
When a chest re-enters network relevancy, the server resends its whole state, old
`SearchingPawn` included; those updates also carry `Role` and are ignored. A chest created
already searched (a dug-up buried chest) still counts when its searcher stands within 5 m.
Pawns resolve to players through `PlayerState` and the lightly obfuscated
`PlayerNamePrivate`.

**Breaks.** A `SearchingPawn` update without `bAlreadySearched` (and not a full-state
resend) is a break by that player; the chest despawns about 5 s later. Opened chests stay
in the world: of 1,901 opens, only 116 despawned within 8 s, and those were broken
afterwards. An older loot-burst heuristic is still available under Settings > Advanced; it
over-counts on current replays.

**Owner and treasures.** `Athena_GameState.RecorderPlayerState` names the recording player.
A prize arrives as a `GA_TreasureHandler_*` element in the `ActivatableAbilities` fast
array of the player's `AbilitySystemComponent` (a stably named subobject). Only the
recorder's abilities replicate in full, and the handler is generic, so the replay shows
when a prize arrived but not which one. Prizes are squad-wide: over 100 replays the
recorder received 39 against about 14 expected from their own 901 chests. Expected drops
therefore use squad chests.

**Game mode.** `CurrentPlaylistInfo` on the game state's `_ClassNetCache` is a custom
delta struct: the delta struct flag, one more bit, then the playlist's NetGUID, which the
stream exports as e.g. `Playlist_NoBuildBR_Squad`. Battle Royale and Zero Build count,
ranked too: `Playlist_Default`, `Playlist_NoBuildBR_`, `Playlist_Habanero` (ranked) and
`Playlist_NoBuildBR_Habanero_` followed by nothing but the team size. Reload and other
islands name the map in between (`Playlist_TimberStakeNoBuildSquad`,
`Playlist_Habanero_Sunflower_Squads`) and don't count; other modes leave the match
list (`matches.json` keeps them under `other_modes` so the scan skips them). A replay
whose playlist can't be read counts, so a parser gap never drops real matches.

**Teammates' replays.** `Athena_GameState.GameSessionId` is the same in every player's
replay of a match, unlike the replay's GUID (one per recording) or its timestamp (the
recorder's clock). For each player, whichever linked replay saw the most of their chests
supplies their whole record.

**Online mode.** `FortPlayerStateAthena.UniqueID` is a `FUniqueNetIdRepl`: a flags byte,
a length byte and the 16-byte Epic account id. The app uploads the recorder's own count
per `GameSessionId` and looks up the account ids of the recorder's squad in the same
match. A squadmate's own number replaces what the recorder's replay saw of them, unless
the replay saw more. The server keeps one record per player per match and answers only
for the exact match and account ids asked about.

### Command line

The exe doubles as a command-line tool:

```
fortnite-chest-counter analyze <replays or folders>... [--nick NAME] [--json]
fortnite-chest-counter info <file.replay>
fortnite-chest-counter dump-events <file.replay>
fortnite-chest-counter list-exports <file.replay> [filter]
fortnite-chest-counter update [--check]
fortnite-chest-counter --minimized
```

`analyze` prints per-match counts, treasures, a chest timeline, who recorded the replay
and the match id. `info` shows the replay's versions and how many network packets decoded
cleanly. `list-exports` lists every replicated class and property, for finding renamed
ones after an update. `update` installs the latest release (`--check` only reports it).

### After a Fortnite update

1. Run `fortnite-chest-counter info` on a new replay and note the engine network version
   and the share of packets that decoded.
2. Look for new version-gated fields in Unreal Engine's `DataChannel.cpp` and
   `NetSerialization.h`, and for renamed chest or vehicle classes (`list-exports`).
3. Add them to `fnreplay` behind `versions.engine_network` checks, or to the schema in
   `crates/chests/src/schema.rs`.
4. Bump `ANALYZER_VERSION` in `crates/app/src/record.rs` so stored matches get re-analyzed.

## Status

Tested on 100+ replays from Fortnite v43.00 (engine network version 45): every packet
decodes. Match ids were checked across two players' replays of the same four matches. The
overlay has been checked over the game in Windowed Fullscreen. The updater was tested end
to end against a local copy of a release, including a download with a wrong checksum.

