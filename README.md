# ROPER

ROPER is a local desktop editor for writing, organizing, and refining rap lyrics on Linux, preferably Debian Trixie. It provides a focused two-pane workspace for developing finished lines alongside loose raw material.

## Overview

ROPER is built as a native GTK 4 application in Rust and is designed for offline, distraction-light writing sessions. It keeps your work on your machine and does not depend on web services, cloud sync, or background accounts.

## Highlights

- Native Linux desktop application with GTK 4
- Dual-pane writing workspace for final lyrics and raw material
- Artist and track organization
- Local artwork support for artists and tracks
- Built-in search
- Automatic saving during editing
- Keyboard-friendly workflow
- Offline-first usage with no runtime network requirement

## Development setup

On Debian Trixie, install the required build dependencies:

```bash
sudo apt-get install rustc cargo libgtk-4-dev pkg-config
```

For the full local verification workflow, also install:

```bash
sudo apt-get install rustfmt rust-clippy
```

## Build and run

Build the project:

```bash
cargo build
```

Run the test suite:

```bash
cargo test
```

Run linting:

```bash
cargo clippy --all-targets --all-features
```

Start the application locally:

```bash
cargo run
```

Build a release binary:

```bash
cargo build --release
```

## Debian package workflow

ROPER includes Debian packaging helpers for Debian Trixie on `amd64`.

Build the installable package:

```bash
./packaging/build-deb.sh
```

Optional metadata overrides for local package builds:

- `DEB_MAINTAINER`, or `DEB_MAINTAINER_NAME` + `DEB_MAINTAINER_EMAIL` to control the Debian maintainer field
- `DEB_HOMEPAGE_URL` to add a homepage to the package/AppStream metadata
- `APPSTREAM_MEDIA_BASE_URL` to expose `roper-metadata.png` and `roper-splash.png` as Discover screenshots via HTTP(S)

Discover and AppStream do not accept packaged local screenshot file paths like `file:///...`; screenshot media must be reachable via HTTP(S).

The generated package is written to:

```text
dist/roper_*_amd64.deb
```

Each package build gets a distinct Debian version suffix, so installing a newer local build upgrades cleanly instead of leaving an older package in place.

Validate the package payload and metadata:

```bash
./packaging/validate-package.sh
```

Install the package locally:

```bash
sudo apt install ./dist/roper_*_amd64.deb
```

Remove the package again:

```bash
sudo apt remove roper
```

## Installed locations

The Debian package installs application files to:

- Binary: `/usr/bin/roper`
- Desktop entry: `/usr/share/applications/org.rmml.roper.desktop`
- AppStream metadata: `/usr/share/metainfo/org.rmml.roper.metainfo.xml`
- App icon: `/usr/share/icons/hicolor/`
- Runtime artwork and bundled SVG assets: `/usr/share/roper/`

## Local data and logs

ROPER now keeps all of its writable local files under a single application root:

- App storage root: `~/.local/share/roper`
- Log file: `~/.local/share/roper/roper.log`

That single root contains settings, artists, ideas, tracks, cached app data, and logs.

If the application cannot start cleanly, check the log file above first.

## Keyboard shortcuts

- `Ctrl+Z`: Undo
- `Ctrl+Shift+Z` or `Ctrl+Y`: Redo
- `Ctrl+C`, `Ctrl+X`, `Ctrl+V`: Copy, cut, paste
- `Ctrl+A`: Select all in the focused editor
- `Ctrl+Enter`: Transfer the active raw line to the final pane
- `Ctrl+F`: Open search
- `Ctrl+L`: Focus the final pane
- `Ctrl+R`: Focus the raw pane
- `F11`: Reassert fullscreen
- `Escape`: Close the active overlay or panel

## Privacy

ROPER is intended for local use. It does not require an online account and does not rely on hosted services during normal use.

## Project status

ROPER is currently published as source code for local development and testing on Linux.

## License

ROPER is released under **The Unlicense**. See `LICENSE` for the full text.
