<div align="center">

<img src="assets/icon/biblewolf-256.png" width="128" alt="BibleWolf icon">

# BibleWolf

**A fast, beautiful Bible study app for Windows, macOS and Linux.**
Read translations side by side, highlight verses, and stick notes on them like post-its.

[![CI](https://github.com/IntelligentWolf-Ltd/biblewolf/actions/workflows/ci.yml/badge.svg)](https://github.com/IntelligentWolf-Ltd/biblewolf/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/IntelligentWolf-Ltd/biblewolf?label=download)](https://github.com/IntelligentWolf-Ltd/biblewolf/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-d9b26a.svg)](LICENSE)

<img src="docs/screenshot.png" alt="BibleWolf showing two KJV panes of John 1 side by side, with John 1:3 highlighted and the Highlights panel open" width="100%">

</div>

## Features

- 📚 **Library tree.** Browse translation → book → chapter, with every chapter one click away.
- 🪟 **Side-by-side panes.** Open up to four translations at once. With **Link panes** on, they all follow the same chapter, and selecting a verse lines the others up on it.
- 🖍️ **Highlighters.** Five colours, applied from the toolbar, a right-click, or the keys **1–5**.
- 🗒️ **Sticky notes.** Notes float over the text like real post-its: drag them, resize them, recolour them, hide them and bring them back. A little marker beside each verse shows where a note is stuck, and the Notes panel lists them all.
- 🔍 **Go to and search in one box.** Type `John 3:16`, `ps 23:1-4`, `1 cor 13` or just `Romans` to jump there, or type any words to search the whole translation.
- 🌗 **Dark "Midnight" and light "Parchment" themes.** Scripture is set in Noto Serif, and the text size is adjustable. Every text colour is tested to meet WCAG AA contrast.
- 💾 **Your notes are safe.** They save automatically and atomically. A damaged file is set aside and never overwritten, and a second open window can't clobber the first one's notes.
- 📦 **One self-contained app.** Eight translations are built in, and it works fully offline.

## Download

Get the latest build from the **[Releases page](https://github.com/IntelligentWolf-Ltd/biblewolf/releases/latest)**:

| Platform | File | Install |
|---|---|---|
| **Windows** 10/11 (64-bit) | `…-windows-x86_64.zip` | Unzip and run `biblewolf.exe` |
| **macOS** 11+ (Apple Silicon & Intel) | `…-macos-universal.dmg` | Open the DMG and drag **BibleWolf** to Applications |
| **Linux** x86_64 / ARM64 | `…-linux-<arch>.tar.gz` | Extract and run `./install.sh` (installs to `~/.local`), or just run `./biblewolf` |

> **First launch on macOS / Windows.** The builds aren't code-signed yet, so the OS warns you the first time:
> - **macOS:** right-click **BibleWolf.app**, choose **Open**, then **Open** again. Alternatively run `xattr -dr com.apple.quarantine /Applications/BibleWolf.app`.
> - **Windows:** on the SmartScreen prompt, click **More info**, then **Run anyway**.
>
> Each release includes `SHA256SUMS.txt` so you can verify your download.

## Translations included

All are in the public domain:

| ID | Translation |
|----|-------------|
| KJV | King James Version (1769) |
| BSB | Berean Standard Bible |
| ASV | American Standard Version (1901) |
| NHEB | New Heart English Bible |
| YLT | Young's Literal Translation (1898) |
| Darby | Darby Bible (1889) |
| Webster | Webster Bible |
| Geneva1599 | Geneva Bible (1599) |

### Add more translations

BibleWolf reads the JSON format from [scrollmapper/bible_databases](https://github.com/scrollmapper/bible_databases), which has 140+ translations in many languages. Download any complete 66-book file from its `formats/json/` folder into your BibleWolf data folder's `bibles/` subfolder (see [Your data](#your-data)), then restart.

## Using it

| Do this | How |
|---|---|
| Open a chapter | Expand a translation and a book in the Library, then click a chapter number |
| Select verses | Click a verse; **Shift-click** another verse to select a range |
| Highlight | Pick a colour in the toolbar or press **1–5**. Press **0**, **Delete** or click **Erase** to remove it |
| Add a sticky note | **📝 Add note**, the **N** key, or right-click a verse |
| Re-open a hidden note | Click its marker beside the verse, or **Show** in the Notes panel |
| Undo deleting a note | **Undo** in the status bar |
| Copy verses | **Ctrl+C** (⌘C on macOS) or **📋 Copy** |
| Change chapter | **← / →** keys, the ◀ ▶ buttons, or the pickers in each pane's header |
| Go to / search | **Ctrl+K**, **Ctrl+F** or **/**, then type and press **Enter** |
| Search for a book name as a word | Put it in quotes: `"John"` |

Highlights and notes belong to the verse, not the translation, so a verse highlighted in the KJV is highlighted in the BSB too.

## Your data

| OS | Folder |
|---|---|
| Linux | `~/.local/share/biblewolf/` |
| macOS | `~/Library/Application Support/biblewolf/` |
| Windows | `%APPDATA%\biblewolf\` |

- `annotations.json` holds your notes and highlights. It's plain JSON, easy to back up or sync. Saves are atomic (temp file, fsync, rename).
- If the file can't be read, it's kept as `annotations.corrupt-<time>.json` and never overwritten.
- If entries point at impossible verses (e.g. after a hand edit), they're skipped and the original is copied to `annotations.invalid-<time>.json`.
- Only one window saves at a time (`biblewolf.lock`). A second window opens read-only and tells you so.
- Window layout and preferences are stored by the UI framework (on Linux, in the same folder).

## Build from source

You need [Rust](https://rustup.rs) 1.89 or newer.

```sh
git clone https://github.com/IntelligentWolf-Ltd/biblewolf.git
cd biblewolf
cargo run --release
```

On Linux, install the windowing headers first. On Debian/Ubuntu:

```sh
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libgl1-mesa-dev libwayland-dev
```

Run the tests with `cargo test`. They cover translation loading, reference parsing, note storage and recovery, the single-window lock, and colour contrast.

### Project layout

```
src/main.rs      window setup
src/app.rs       UI: library tree, panes, highlighter, sticky notes, search, status bar
src/bible.rs     translation loading and reference parsing ("1 cor 13:4-7")
src/store.rs     notes & highlights persistence
src/theme.rs     palettes, fonts, contrast tests
tools/           converters for Bible data and the app icon
packaging/       Linux .desktop entry, macOS Info.plist
```

The built-in Bibles are stored as gzipped TSV, generated by `tools/convert_scrollmapper.py` from scrollmapper's JSON.

### Releasing

Bump `version` in `Cargo.toml`, commit, then tag and push:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

GitHub Actions builds Windows, macOS (universal) and Linux (x86_64 + ARM64) packages and publishes them as a release.

## Contributing

Issues and pull requests are welcome. Please run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` before opening a PR; CI checks all three on Linux, Windows and macOS.

## License

**BibleWolf** is © 2026 [IntelligentWolf Ltd](https://wolf.uk.com) and released under the [MIT License](LICENSE).

Bundled material keeps its own terms:
- Bible texts: public domain, via [scrollmapper/bible_databases](https://github.com/scrollmapper/bible_databases).
- Noto Serif fonts: [SIL Open Font License 1.1](assets/fonts/OFL.txt).
- The app icon is drawn from the Noto Emoji wolf: SIL Open Font License 1.1.
- Built with [egui](https://github.com/emilk/egui).
