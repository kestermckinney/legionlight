# Legion Light

Keyboard backlight control for the Lenovo Legion 5 15AHP11 (`83Q7`), in Rust with an optional GPUI desktop app.

The laptop's firmware confirms **one RGB zone**. Static colour and brightness
have been confirmed in both the diagnostic command and the GUI. Effects and
profile encodings come from Lenovo's Windows installer; see
[installer evidence](docs/installer-analysis.md).

## Install with Flatpak

Requires [Flatpak](https://flatpak.org/setup/) with the Flathub remote (for the
runtime). Add the Legion Light repository, then install and run:

```bash
flatpak remote-add --user --if-not-exists legionlight \
  https://kestermckinney.gitlab.io/legionlight/flatpak/legionlight.flatpakrepo
flatpak install --user legionlight io.gitlab.kestermckinney.LegionLight
flatpak run io.gitlab.kestermckinney.LegionLight
```

Update with `flatpak update`. Or install the `LegionLight.flatpak` bundle from the
latest release with `flatpak install --user LegionLight.flatpak`.

The Flatpak cannot grant it permission to access `/dev/hidraw*` without additional setup:
see https://github.com/kestermckinney/legionlight/issues/130 for details.
A manual workaround is available via:

```bash
sudo udevadm control --reload-rules
```

## Features

- Full control over keyboard backlight settings
- Support for static colors, pulse effects, and brightness levels
- GPUI desktop application for full customization
- A GNOME top-bar extension with idle dimming, packaged separately (see below)
- Compatible with Flatpak packaging

## GNOME Extension

The GNOME Shell extension (top-bar icon, settings dialog and idle dimming daemon)
lives in its own project with its own Flatpak:
[Legion Light for GNOME](https://gitlab.com/kestermckinney/gnomelegionlight).
It uses this crate's `legion_light` library to drive the keyboard. Do not run the
desktop app and the GNOME daemon at the same time.

## Technical Details

This project consists of several components:
1. **Command-line tool** (`legion_light_ctl`) - core functionality for interacting with the device
2. **GUI desktop app** (`legion_light`) - full-featured interface using GPUI  
3. **Libraries** - shared code for lighting protocols, models, etc.

### Building

```
# Build all components (requires Rust)
cargo build --release
```

### Installation

Installation requirements are the same as the main Flatpak application. For development:
- Set up udev rules if you're not using the Flatpak version

## Development

This software is written in Pure Rust with no Python, and builds clean without any C/C++ code.
See `DESIGN.md` for development and design notes.

## License

MIT - see [LICENSE](LICENSE) file for full text.