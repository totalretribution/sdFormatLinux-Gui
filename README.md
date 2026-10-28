# sdFormatLinux-Gui

A minimal Linux GUI for [sdFormatLinux](https://github.com/profi200/sdFormatLinux). Formats SD cards as FAT32 with an optional volume label.

## Features

- Lists block devices (no partitions, no loop devices)
- Optional volume label
- Streams formatter output in real time
- Authenticates via `pkexec` once per session — root shell is reused for subsequent formats
- Always passes `-f` (force FAT32)
- Checks GitHub releases for a newer version at most once a day and shows a link when one is out (never updates itself). Set `SDFORMAT_GUI_NO_UPDATE_CHECK=1` to turn this off

## Requirements

- [`sdFormatLinux`](https://github.com/profi200/sdFormatLinux) — bundled privately in the `.deb`/`.rpm`; source builds need it in `$PATH`
- `pkexec` (polkit)
- X11 or Wayland

## Install

Download the `.deb` or `.rpm` from the [releases page](../../releases) and install:

```bash
# Debian / Ubuntu
sudo dpkg -i sdformatlinux-gui_*.deb

# Fedora / RHEL
sudo rpm -i sdformatlinux-gui-*.rpm
```

## Build from source

```bash
# System dependencies (Debian/Ubuntu)
sudo apt-get install libgl-dev libx11-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev pkg-config

cargo build --release
./target/release/sdformatlinux-gui
```

Source builds do not include `sdFormatLinux` — build it from the [upstream repo](https://github.com/profi200/sdFormatLinux) (`make release`) and put it in your `$PATH`.

## Third-party software

The `.deb` and `.rpm` packages include [sdFormatLinux](https://github.com/profi200/sdFormatLinux) v0.2.0, built from source at the upstream release commit and installed privately at `/usr/libexec/sdformatlinux-gui/sdFormatLinux` (not on `$PATH`). The app uses this copy when present and falls back to `sdFormatLinux` in `$PATH` otherwise. sdFormatLinux is Copyright (c) 2023 profi200 and distributed under the MIT License. Its licence text is installed at:

- `/usr/share/doc/sdformatlinux-gui/sdFormatLinux-LICENSE.txt` (Debian / Ubuntu)
- `/usr/share/licenses/sdformatlinux-gui/sdFormatLinux-LICENSE.txt` (Fedora / RHEL)

## Usage

1. Select device from the dropdown (refresh with ↺)
2. Optionally enter a volume label
3. Click **Format SD Card** — pkexec prompts for your password once
4. Output streams into the log area; button re-enables when done

Subsequent formats in the same session reuse the root shell — no further password prompts.
