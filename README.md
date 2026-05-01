# sdFormatLinux-Gui

A minimal Linux GUI for [sdFormatLinux](https://github.com/profi200/sdFormatLinux). Formats SD cards as FAT32 with an optional volume label.

## Features

- Lists block devices (no partitions, no loop devices)
- Optional volume label
- Streams formatter output in real time
- Authenticates via `pkexec` once per session — root shell is reused for subsequent formats
- Always passes `-f` (force FAT32)

## Requirements

- [`sdFormatLinux`](https://github.com/profi200/sdFormatLinux) in `$PATH`
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

## Usage

1. Select device from the dropdown (refresh with ↺)
2. Optionally enter a volume label
3. Click **Format SD Card** — pkexec prompts for your password once
4. Output streams into the log area; button re-enables when done

Subsequent formats in the same session reuse the root shell — no further password prompts.
