# Guide

How to use Fortnite Chest Counter. The short version is in the [README](../README.md);
how it counts and how to build it are in [development.md](development.md).

## Install and updates

Download `fortnite-chest-counter.exe` from the [latest release](../../../releases/latest)
and run it. Windows 10 or 11, 64-bit; no installer. Turn on **Settings > Game > Replays >
Record Replays** in Fortnite. The first run puts a shortcut on your desktop, so move the
exe where you want to keep it first.

When a new version is out, the app installs it by itself once Fortnite is closed and its
window is in the tray (Settings > About > Install updates automatically). To update right
away, press **Update to vX** in the sidebar. Your matches and settings stay. What changed in each version:
[changelogs/](../changelogs/).

## First run

Three short steps: the language, where to start counting, and what runs in the
background. The first step also says how many replays the app found. If it found none,
check that Record Replays is on in Fortnite. To start from the quest, enter
the count the game showed and when it stopped counting (still on the quest? its current
progress and the current time). Not sure when it finished? See
[When did my quest finish?](#when-did-my-quest-finish)

You don't need to enter your Fortnite name: the app reads it from your replays.

## Pages

- **Overview**: your total, your squad's, your chance of Pirate Shop Access, treasures
  received, and the next Treasure Hour or the one running. A notification comes 15 minutes
  before one starts, and the in-game overlay says TREASURE HOUR while it runs; chests
  looted in it count with their boost in the Pirate Shop Access chance. Settings > General
  turns each of these off.
- **Matches**: every Battle Royale and Zero Build match, ranked included, with a
  timeline of each chest: opened, broken with a pickaxe or rammed with a vehicle, and the
  prizes it gave: which one, when, and whose chest. Other modes, such as Reload and
  Creative, don't count and aren't listed. Prizes
  other players got near you are marked "Not you" and count nowhere.
- **Stats & odds**: chests by type, chests needed for a 25/50/90% chance, how lucky you've
  been.
- **Map**: where you looted, and with Online mode, where squadmates who run the app did.
  Click a marker to see which chest it was and when.
- **Squad replays**: add your teammates' replays to fill in what yours missed.
- **Leaderboard**: everyone with Online mode, for all time, this week or today.
- **Settings**: language, theme, autostart, replay folder and archive, overlays, Online
  mode.

The app runs in the tray and counts each match when you're back in the lobby. By default it
starts when Fortnite does and waits in the background without a window.

**Overlays.** The in-game overlay (Settings > Overlay) shows your count on top of the game;
click its lock to move and resize it. The OBS overlay (Settings > OBS and Discord) is a
Browser source at `http://127.0.0.1:47321/`, size 400 x 220, transparent background. The
same section sets what your Discord status shows while you play.

**Several accounts.** When a replay comes from an Epic account the app hasn't seen on
this PC, it asks whether it's yours. Each of your accounts gets its own count; switch at
the top of the menu. Settings > Counting > Other accounts on this PC changes the answer
later, and nothing is deleted either way.

**Missing matches.** If a Battle Royale match ends without a replay, the app warns you.
Fortnite keeps only the latest 100 replays, so the app keeps copies of the ones it counted
(up to 10 GB; Settings > Replays). Chests from matches already gone can be added by hand
in Settings > Counting.

Data lives in `%APPDATA%\Fortnite Chest Counter`.

## Squad chests

A replay only records what happened near you, so squad numbers are a lower bound
("152+"). Two ways to fill them in:

- **Online mode** (on by default): squadmates who run the app with Online mode on swap
  their chests with you after each match: the exact count, and each chest on the Map in
  their own color. No friend list or code needed.
- **Their replays**: ask for the files from `%LOCALAPPDATA%\FortniteGame\Saved\Demos` and
  drop them on the Squad replays page. Nothing gets counted twice.

## When did my quest finish?

Fortnite shows that a quest is done but not when. Your Epic profile has the exact time:

1. In **Settings > Counting**, expand *Not sure when your quest stopped counting? Get the
   exact time*, click **Open the script folder** and run `get-epic-profile.bat`.
2. Sign in on the Epic page that opens and copy the line with `authorizationCode`.
3. Paste it into the script window and press Enter. It saves `epic-profile.json`.
4. In the app, click **Import epic-profile.json**.

The app fills in and saves your starting count and time. Delete `epic-profile.json` afterwards: it
holds your account's game data. The app itself never gets access to your Epic account;
it only reads that file. The script is open:
[get-epic-profile.ps1](../crates/app/scripts/get-epic-profile.ps1).

## Privacy

- The app reads replays and Fortnite's log on your PC. It doesn't touch the game, its
  memory or its network traffic.
- **Online mode** (on by default; Settings > Online turns it off) sends `chests.l4ve.dev`
  your Epic account id and name, for each match its id, start time and your chests (each
  one with when, what kind and where on the island it was), your total, whether to hide
  your name in the leaderboard, the app version and a random id of this install.
  Squadmates get your chests only for matches you played together. The
  leaderboard shows your name (or Anonymous), matches, chests and best match, never ids.
  No replays, no other players' names, no analytics.
- **Discord status** (on by default; Settings > OBS and Discord) goes to the Discord app
  on this PC, which shows it on your profile while Fortnite runs. The app never signs in
  to Discord.
- **The OBS overlay** answers only this PC.
- **Updates**: the app asks GitHub for the latest release every 10 minutes, and for the
  list of prize names and Treasure Hours every 6 hours. Nothing about you is sent.

## Known limitations

- A buried chest can occasionally be missed: now and then the replay lacks the data
  needed to read it.
- The replay names the weekly prizes and Pirate Shop Access, not gizmos (golden
  fireworks, extractors and the like): those show as "Gizmo".
- The in-game overlay needs Windowed Fullscreen.
- A Fortnite update can change the replay format. If the Overview warns that replays
  didn't decode cleanly, wait for an app update.
