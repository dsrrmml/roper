# ROPER

ROPER is a native RUST/GTK4 desktop app for Linux (tested on Debian Trixie) focused on turning rough idea fragments into finished lyrics.

This documentation covers:

- WHY IT WAS BUILT
- PRACTICAL WORKFLOW
- HOW HIGHLIGHTING WORKS
- FEATURES
- INTENDED IDEAS WORKSPACE FLOW
- PACKAGING && LOCAL DATA LAYOUT
- TODO
- BUGS
- INCOMING

---

## WHY IT WAS BUILT

I needed a distractionless (yeah, even breathing distracts me) editor
for developing text ideas over time to final lyrics for various projects.

Since i never did some app-dedicated Rust and got some fresh Pink Kush
at the same time, the project started.

I needed an (for me) distractionless text editor with minimal feature-set,
font-property- and formatting-wise.

For me the main features where:

- the editor has to be open in fullscreen, no visible border to the screens
edges of the visible area.

- only UPPERCASE/lowercase, Preserving-case-mode and font-size as formatting 
tools.

- two-pane editor-mode for separate raw- and final version of the lyrics.
I like to have a pool of loosely gathered text material on the left side,
labelled as "raw" and then compose/condense that into final lyric material
on the right side, labelled as "final".

- three-pane editor-mode for developing ideas, because sometimes i have ideas
for an hook- or bridge-part and at the same time i do not want to have that
in the same visible, vertical lane as the currently developing idea.

- the editor should automatically save all data entering its editor panes, 
i will always able to close the editor, re-open it and continuing on the
last state i left it.

- working with my individual project folder style. yeah. trippy.

I do already all my lyrics work with that app already and note every bug
and wanted improvements on that way.

When i was in anger because of some Ardour fuckups in between,
i've decided to call it "ROPER".

---

## What ROPER is built for

Core use-case is developing final lyrics from a heap/pool of raw ASCII text
material. Everything which is available in the raw.txt file inside the selected
track's lyrics subfolder gets shown in the left "raw" editor pane.

Everything which one considers as "final" is available in the final editor pane
and therefore the content of the projects lyrics subfolders final.txt file.

One can start by having an idea for a line/bar/verse/part/hook/bridge/, intro or
outro and can use the "IDEAS" three-pane editor mode for that:

1. *** IN/OUT *** (left) - separates the intro/outro of an track idea from the rest.
2. *** VERSES *** (cente) - focus on the core part of the track.
3. *** HOOKS/BRIDGES *** - something which might change later on more often and serialized.

After creating a track, eventually outside of the app and adding it afterwards, the material of the idea can be transferred to the new track. 

Then it works probably best when one writes in two phases, during finalizing phase:

1. *** RAW *** (left): free-form material, fragments, alternates, punchline variants.
2. *** FINAL *** (right): structured song with sections and the wanted structure.

One can keep massive drafts and still navigate relatively fast due:

- live repeat highlighting
- structure highlighting (`[INTRO]`, `[VERSE 1]`, `[HOOK]`, `[OUTRO]`, ...)
- minimap + viewport indicator for vast lyrics material
- line-indexed raw gutter actions (when they behave)

---

## Core workflow patterns

### Workflow A: from blank page to structured draft

1. Create/select artist and track.
2. Dump ideas in the **raw pane** rapidly.
3. Promote lines into **final pane** and add structure tags
with the "STRUCTURING-TOOL":
	- `[INTRO]`
	- `[VERSE 1]`, `[VERSE 2]`, ...
	- `[HOOK]` or `[HOOK 1]`, `[HOOK 2]`
	- `[OUTRO]`
4. Use visual, highlighting (tags,numbers,empty lines) feedback to reduce repetitive weak or "cold" spots.
5. Use minimap to jump large sections quickly.

### Workflow B: from idea to raw-to-final iterative polishing

1. First develop ideas in the ideas 3 editor panes.
2. Create/select artist and track.
3. Transfer developed idea/s into the freshly created track.
4. Use the pre-developed lyrics from the "RAW" section.
5. Keep alternates in raw lines.
6. Mark and transfer only the strongest material to "FINAL".
7. Let repeat heatmap gutter (_1_) expose overused words in the final pane.
8. Rewrite while preserving cadence and structure until "finished"

_1_ (not yet developed feature....lel)

### Workflow C: ideas-first writing sprint

1. Open ideas workspace.
2. Fill all three panes:
	- **IN/OUT** (framing, entry/exit, intro/outro language)
	- **VERSES** (technical bars and progression)
	- **HOOKS/BRIDGES** (anchors and transitions)
3. Transfer selected idea content into a already existing track.
4. Continue arrangement and cleanup in the split editor like nothing happened.

---

## How highlighting works (important)

ROPER has multiple highlight channels that support decision-making while writing.

### 1) Raw chain highlights (orange, left pane)

- ROPER tokenizes words from both panes.
- If a word appears in final text, matching spans in raw are marked in orange.
- This gives immediate visual feedback on what source material is already represented.

Practical use:

- avoid reusing the same pool blindly
- quickly see untouched raw clusters worth mining

### 2) Final repeat heatmap (red intensity, right pane)

- Repeated words in final pane are shaded with increasing red intensity.
- Nearby repetitions receive stronger buckets than distant repetitions.

Bucket behavior (conceptual):

- very close repeats -> hottest red
- medium distance repeats -> medium red
- far repeats -> lighter red

Practical use:

- keep hooks intentional while reducing accidental repetition
- spot lazy fallback words in dense sections

### 3) Warning markers (skull icons)

- Dense repetition zones in final text produce warning icons.
- They are drawn near line ends or inline when space allows.
- In current UI behavior, warnings are rendered as skull markers for visibility.

Practical use:

- quickly inspect high-risk lines for monotony
- yeah, sometimes its needed for hooks..have to resolve that issue
- force rewrite passes where lexical variety collapses

### 4) Structure highlights (section-colored ranges)

Detected tags color full regions until the next tag:

- `[INTRO]`
- `[VERSE n]`
- `[HOOK]` / `[HOOK n]`
- `[BRIDGE]` / `[BRIDGE n]`
- `[OUTRO]`

Practical use:

- maintain structural readability in long tracks
- recognize doubled/stale/redundant sections 
- reduce accidental section bleed

### 5) Minimap + section badges (for long text)

- Minimap appears when pane content exceeds line threshold.
- It visualizes line density and structure blocks.
- Viewport rectangle shows what part is currently visible.
- Clicking/dragging minimap scrolls directly.

Practical use:

- jump from early verse to late bridge instantly
- keep orientation in 200+ line drafts

---

## Built-in showcase dataset

Repository includes a generator that populates real app storage with heavy demo content:

- **3 artists**
- **6 tracks** (2 per artist)
- each track has **200+ lines** in Romanian in both raw/final states
- populated **used-material** markers for gutter features
- **3 ideas** with substantial text in all three idea panes
- generated artist images + track artwork
- generated screenshot set (8 files)

### Generate/populate showcase data

Run:

```bash
python3 scripts/populate_showcase_data.py
```

Default storage target:

- `~/.local/share/roper`

Override target root if needed:

```bash
ROPER_STORAGE_DIR=/absolute/path/to/roper-data python3 scripts/populate_showcase_data.py
```

---

## Keyboard shortcuts

- `Ctrl+Z`: Undo
- `Ctrl+Shift+Z` / `Ctrl+Y`: Redo
- `Ctrl+C`, `Ctrl+X`, `Ctrl+V`: Copy / Cut / Paste (Probably fucks up some os-intended behaviour)
- `Ctrl+A`: Select all in active editor
- `Ctrl+Enter`: Transfer active raw line
- `Ctrl+F`: Search
- `Ctrl+L`: Focus final pane
- `Ctrl+R`: Focus raw pane
- `Escape`: Close active search/overlay/panel
- `F11`: Reassert fullscreen

---

## Development setup

On Debian Trixie:

```bash
sudo apt-get install rustc cargo libgtk-4-dev pkg-config
```

Optional tooling:

```bash
sudo apt-get install rustfmt rust-clippy
```

### Build and test

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features
```

Run locally:

```bash
cargo run
```

---

## Debian package workflow

Build package:

```bash
./packaging/build-deb.sh
```

Validate package:

```bash
./packaging/validate-package.sh
```

Install local build:

```bash
sudo apt install ./dist/roper_*_amd64.deb
```

Remove again:

```bash
sudo apt remove roper
```

Generated artifact pattern:

- `dist/roper_*_amd64.deb`

---

## Installed paths (Debian package)

- Binary: `/usr/bin/roper`
- Desktop file: `/usr/share/applications/org.rmml.roper.desktop`
- AppStream metadata: `/usr/share/metainfo/org.rmml.roper.metainfo.xml`
- Icons: `/usr/share/icons/hicolor/`
- Runtime app assets: `/usr/share/roper/`

---

## Local data model

ROPER stores writable runtime state in one root:

- `~/.local/share/roper`

Important subpaths:

- `artists/` (artist metadata files)
- `artist_images/` (artist image files)
- `tracks/<track-id>/settings.json`
- `tracks/<track-id>/lyrics/final.txt`
- `tracks/<track-id>/lyrics/raw.txt`
- `ideas/<idea-id>/settings.json`
- `ideas/<idea-id>/{in_out,verses,hooks_bridges}.txt`
- `roper.log`

---

## Privacy model

ROPER is designed for local/offline usage and does not require cloud accounts or hosted runtime services.

---

## License

ROPER is released under **The Unlicense**.
See `LICENSE` for full text.
