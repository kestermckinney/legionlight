# LegionForGnome — Design

A Rust + GPUI application for the Lenovo Legion 5 15AHP11 (`83Q7`) that controls
the keyboard backlight and the handful of other settings the Windows "Legion
Space" utility exposes.

Status: **the GPUI desktop app (`legion_light`) is the product.** The Phase 0/1
command-line tool and the C/Python probes that established the protocol have been
removed; their findings remain in [`docs/protocol-notes.md`](docs/protocol-notes.md)
and [`docs/installer-analysis.md`](docs/installer-analysis.md). Sections below that
describe a CLI or `sysfs` module are historical design notes.

Everything in the repository is Rust. There is no Python or C in the product, the
test suite or the packaging.

---

## 1. What the Windows utility actually does

`legionspace-1.9.11.5_x64_rtm.exe` is a 592 MB bootstrapper. Its `.rsrc` section
carries two archives (`ZIPRES/108` = 608.8 MB 7-Zip payload, 5,050 files;
`ZIPRES/107` = 7.1 MB installer theme ZIP) plus three helper DLLs. The installed
product is an Electron/CEF shell plus native "Smart Engine" plugins.

The lighting chain is:

```
LegionSpace.exe                      Electron/CEF UI
  → LenovoLightingService.exe        ATL service "Lighting Driver Service",
                                     named pipe + registry state,
                                     LightingStatusManager, CPipelineClient
    → SELuminousNB64.dll             plugin, moduleid 2015, contract SELuminousNB
                                     imports WINUSB.DLL + HID.DLL + SETUPAPI.dll
      → ITE 8258 USB HID controller  vendor usage page 0xFF89
```

Two conclusions matter:

1. **The control path is raw USB HID, not WMI.** `SELuminousNB64.dll` imports
   `WinUsb_WritePipe` / `WinUsb_ReadPipe` / `WinUsb_ControlTransfer` and
   `HidD_GetPreparsedData`. The WMI path (`lenovo_wmi_gamezone`) exists on this
   machine but is *not* how the keyboard lighting is driven. This is good news:
   the protocol is reachable from Linux without a reverse-engineered ACPI method.
2. **Lenovo ships a second, standard interface.** `SparsePackage/Lightingpackage.msix`
   is a sparse MSIX named `LegionLightingController` that registers the
   `com.microsoft.windows.lighting` contract (Windows 11 Dynamic Lighting). So the
   keyboard also speaks an open HID LampArray protocol. That is a fallback path,
   not the rich one — see §4.

The plugin also carries a full effect model: layout configs
(`keyboard_24zone.json`, per-key `subdevice.json` with 216 lamps, 4-zone
variants), profile configs (`profiles[].layers[].animationConfig` with
`animationId`, `colorList`, `speed`, `direction`, `clockwise`, `keys[]`), and
`brightness.json` (`{"brightness": 7}`). Lenovo's own UI therefore talks in
*profiles and effects*, but on the wire it is a report of zones + colours +
brightness. We reproduce the wire format, not the effect taxonomy.

---

## 2. The hardware, as verified on this laptop

| Property | Value |
|---|---|
| DMI product / board | `83Q7` / `LNVNB161216` |
| USB ID | `048d:c615` "ITE Device(8258)" |
| Bus / address | USB 1, `1-4` |
| Interface 00 | HID, usage page `0xFF89`, usage `0x07` (vendor lighting) |
| Interface 01 | HID, usage page `0x59` (LampArray) |
| Nodes | `/dev/hidraw0` (iface 00), `/dev/hidraw1` (iface 01) |
| Kernel | 7.2.15-300.fc45.x86_64, `hid-generic` binds both interfaces |
| Endpoints | one interrupt IN per interface, 64-byte max packet |

Interface 00 feature reports (parsed from the report descriptor):

| Report | Payload | Role |
|---|---|---|
| `0x07` | 959 B (+1 ID = 960 B buffer) | the lighting command channel |
| `0x5A` | 16 B | device/state query |
| `0xCC` | 32 B | legacy 4-zone channel (write-only in practice) |

Interface 01 feature reports `0x01`–`0x06` are the LampArray set (attributes,
attributes request/response, multi-update, range update, control).

Read-only probes that were run:

- `GET_FEATURE 0x07` → `07 03 09 00 01 00 55 00` + zeros. A firmware/version
  reply, **not** a colour buffer.
- `GET_FEATURE 0x5A` → `00 ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff`
- `GET_FEATURE 0xCC` → `EPIPE` (reads are not accepted; it is an output path)
- LampArray attributes: `LampCount = 1`, bounding box 333 × 119 × 3 mm,
  `LampArrayKind = 1` (keyboard), min update interval 2400 µs; lamp 0 is
  programmable with 255 RGB levels, 1 intensity level, 4 ms latency, purpose
  `Control`.

The one-lamp LampArray is the important limitation: **interface 01 can only set
one uniform colour for the whole keyboard.** The 24-zone behaviour lives on
interface 00.

---

## 3. The backlight protocol (interface 00, report `0x07`)

A 960-byte feature report. Envelope:

```
byte 0        0x07                     report ID
byte 1        command                  see below
byte 2..3     payload length, LE u16
byte 4..       payload
```

Validated commands (on the sibling controller `048d:c195`, identical descriptor
family):

| Cmd | Payload | Meaning |
|---|---|---|
| `0xC8` | `01` | select profile 1 |
| `0xCB` | zone/colour groups | write the zone table |
| `0xCE` | `01`..`09` | brightness |
| `0xCB` | `01 01 01` | blank table → lights off |

A static colouring is the three-report sequence:

```
1. 07 C8 01 00 | 01
2. 07 CB <len> | 01 01 01 <groups...>
3. 07 CE 01 00 | <1..9>
```

Each group is a small property block, emitted once per distinct colour:

```
[group_id][0x06][0x01 0x0B][0x02 0x02][0x03 0x00][0x04 0x00]
[0x05 0x02][0x06 0x00][0x01][R][G][B][count][led_id LE u16]... [led_id LE u16]
```

`led_id` is 1-based, 1..24. The `0x02`–`0x06` key/value pairs and the `0x01 0x0B`
header are **opaque**: we reproduce them byte-for-byte rather than invent
meaning for them. That is deliberate — those bytes are the only thing standing
between "works" and "device ignores the report".

Descriptor matching must be a **substring** test, not a prefix test. Our
descriptor begins with the `0x5A` collection and only reaches the lighting
collection at offset 23:

```
06 89 FF 09 10 A1 01 85 5A ...        ← header collection
C0 06 89 FF 09 07 A1 01 85 07 ...     ← lighting collection at offset 23
```

The Python reference project (`ulrickpsp/legion-control-linux`, MIT) validates
with a prefix check because its unit happens to start with the lighting
collection. We must use `signature in descriptor` or we will reject our own
device. It also means the byte layout must be re-derived in Rust rather than
translated line-by-line from a prefix-based implementation.

**Confidence:** the encoder is byte-identical to a project that validates
writes on `048d:c195`, and our descriptor contains the same signature and the
same report IDs. It is *not yet proven* on `c615`. §11 makes proving it the first
task.

---

## 4. Interface 01 (LampArray) — what it is and is not

`GET_FEATURE 0x01` reports one lamp covering the whole keyboard, so the standard
`LampMultiUpdateReport` can do exactly one thing on this laptop: set a single
colour for all keys, plus on/off via `LampArrayControlReport`.

That is still worth having:

- it is a **documented, stable** protocol (USB HID Usage Tables, Lighting and
  Illumination page `0x59`; Microsoft's Dynamic Lighting reference descriptor);
- it is what a future in-kernel `hid-lamparray` driver and any desktop-level
  lighting integration will speak;
- it is a safe fallback if the vendor report ever changes.

It is not the primary path, because 24 zones > 1 lamp. Design decision:
**interface 00 is primary, interface 01 is an optional compatibility backend.**
Never treat one as a substitute for the other.

---

## 5. "Other options"

Verified available on this machine (`83Q7`, kernel 7.2.15):

| Capability | Interface | Current value | Writable |
|---|---|---|---|
| Platform profile | `/sys/firmware/acpi/platform_profile` (+ `_choices`: `low-power balanced performance`) | `performance` | root |
| Battery conservation | `/sys/bus/platform/devices/VPC2004:00/conservation_mode` | `1` | root |
| Fn-lock | `…/VPC2004:00/fn_lock` | `0` | root |
| Fan mode | `…/VPC2004:00/fan_mode` | `0` | root |
| Fan RPM | `hwmon1` (`acpi_fan`), `hwmon9` (`yogafan`) | ~2000 rpm | read-only |
| Temperatures | `hwmon8` (`k10temp`), `hwmon7` (`amdgpu`), `hwmon4` (`nvme`) | — | read-only |
| AC / battery | `hwmon0` (`ACAD`), `hwmon3` (`BAT1`) | — | read-only |

Not available here: `lenovo_wmi_other` is bound (`DC2A8805-…`) but publishes no
fan-target, `ppt_pl1_spl` or `ppt_pl2_sppt` attributes, so **custom fan curves and
CPU power limits are off the table** on this SKU. `lenovo_wmi_gamezone` exposes
only `platform-profile`. There is no `charge_control_end_threshold`; conservation
mode is the battery mechanism.

So "other options" honestly reduces to four switches plus telemetry. Anything
beyond that would require WMI/ACPI work that this hardware does not appear to
expose.

---

## 6. Toolchain: Rust + GPUI, following SeeMyMac

The application is a single Rust crate with a `lib` and one `[[bin]]`
target, exactly like `../SeeMyMac`: `src/main.rs` opens the window, `src/app.rs`
is the controller, `src/view.rs` is rendering, pure logic lives in modules that
never import `gpui`, and `src/ui_tests.rs` drives the real view through
`gpui::VisualTestContext`.

```toml
[features]
gui = ["dep:gpui"]                 # the library builds without GPUI

[[bin]]
name = "legion_light"
path = "src/main.rs"
required-features = ["gui"]
```

Deliberate choices:

- **`libc` and not `hidapi`.** The safety gate needs `HIDIOCGRAWINFO`,
  `HIDIOCGRDESC` and `HIDIOCSFEATURE(len)` with an exact, caller-computed length.
  `hidapi` exposes none of those. Four `ioctl` calls and one `open` with
  `O_NOFOLLOW|O_CLOEXEC` are the whole transport, and there is no C dependency or
  version skew to negotiate.
- **`async-channel` + `futures-lite`**, the same wake mechanism SeeMyMac uses:
  the hardware thread pushes events, a `cx.spawn_in` loop selects on the channel
  and a 33 ms timer, and calls `cx.notify()`. No polling of the device.
- **No `zbus` in the app.** The GUI never talks D-Bus. It either writes hidraw
  directly (Phase 2, after a `uaccess` udev rule) or, from Phase 3, calls the
  daemon — through a thin client module, never inline in the UI.
- **No `nix`.** Its ioctl macros assume fixed-size requests; `HIDIOCSFEATURE` is
  computed per length. `libc::ioctl` with a local `const fn` is smaller than the
  workaround.

### GPUI constraints to design around

These are real and shape §8:

- **No native widgets.** Every control is a `div()` with `Stateful<Div>` handlers
  — the same `tool_btn` / `primary_btn` / `text_field` helpers SeeMyMac builds in
  `view.rs`. There is no slider, switch, colour dialog, or tab widget to reuse.
- **No built-in colour picker.** We draw one: a hue strip and a
  saturation/value square, using `linear_gradient()` for the gradients and
  `hsla()` for the values, with `on_mouse_down` + `on_drag_move` for the handles.
- **Client-side decorations on Linux.** SeeMyMac's `NATIVE_FRAME` pattern gives a
  GNOME-style header bar drawn by the app; we copy it, with
  `window_decorations: Client`, `app_id: Some("legionlight")`, and a transparent
  window background.
- **Text entry is hand-rolled** (focus handle + key handler). Used only for
  preset names, so it can be one small, tested component.

---

## 7. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ legion_light  (gpui window, main thread)                     │
│   App: LightingState + presets + selected zones              │
└──────────────┬───────────────────────────────────────────────┘
               │ Command (async-channel)      ▲ Event
               ▼                             │
┌──────────────────────────────────────────────────────────────┐
│ lighting worker thread                                       │
│   owns the hidraw fd, validates identity, rate-limits,       │
│   encodes + writes reports, reports applied/denied state     │
└──────────────┬───────────────────────────────────────────────┘
               │  ioctl(HIDIOCSFEATURE)          sysfs reads
               ▼                                     ▼
     ITE 8258 (048d:c615)                 hwmon / power_supply
```

Module layout (single crate):

```
src/lib.rs
src/main.rs                 entry: window options, app_id, assets
src/app.rs                  App controller (gpui Entity), channels, actions
src/view.rs                 rendering + design tokens
src/panels.rs               Lighting / Performance / Battery / About pages
src/ui_tests.rs             #[gpui::test] visual tests
src/settings.rs             persisted state, dirs::config_dir()
src/model.rs                LightingState, Preset, ZoneId, Rgb — no gpui types
src/lighting/mod.rs
src/lighting/protocol.rs    report encoder — pure, no I/O
src/lighting/descriptor.rs  HID report-descriptor reader — pure
src/lighting/hid.rs         discovery, identity gate, ioctl transport
src/lighting/worker.rs      thread, Command/Event, rate limiter      (Phase 2)
src/lighting/zone_map.rs    logical zone ↔ UI region, calibration    (Phase 4)
```

Implemented today: `main`, `app`, `view`, `ui_tests`, `model` and
`lighting::{protocol, descriptor, hid, palette, worker}`. `panels`, `settings` and
the `(Phase 4)` module are planned, not present.

**`model.rs` and `lighting/protocol.rs` import no `gpui` and no `libc`.** They are
unit-tested on any machine, hardware or not. That split is what makes the protocol
testable and the UI testable, independently.

### The transport

```rust
// src/lighting/hid.rs
const fn hid_ioc(dir: u64, nr: u64, len: usize) -> libc::c_ulong {
    (dir << 30 | (len as u64) << 16 | (b'H' as u64) << 8 | nr) as libc::c_ulong
}
const fn hid_iocsfeature(len: usize) -> libc::c_ulong { hid_ioc(3, 0x06, len) }
const fn hid_iocgfeature(len: usize) -> libc::c_ulong { hid_ioc(3, 0x07, len) }

pub struct Device {
    fd: OwnedFd,
    path: PathBuf,
}

impl Device {
    /// Open only when the node really is the 048d:c615 lighting interface.
    pub fn open_validated(path: &Path) -> Result<Self, DeviceError> {
        // O_NOFOLLOW: a symlinked /dev/hidraw0 must not become "write to anything".
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let fd = OwnedFd::from(file);

        let (bus, vid, pid) = hidraw_info(&fd)?; // HIDIOCGRAWINFO, 3x u16
        if (bus, vid, pid) != (0x03, 0x048D, 0xC615) {
            return Err(DeviceError::WrongDevice { bus, vid, pid });
        }

        let desc = report_descriptor(&fd)?; // HIDIOCGRDESC, len-prefixed buffer
        if !desc.windows(SIGNATURE.len()).any(|w| w == SIGNATURE) {
            return Err(DeviceError::WrongProtocol); // substring, not prefix
        }
        Ok(Self { fd, path: path.into() })
    }

    pub fn send(&self, report: &[u8; 960]) -> io::Result<()> {
        let mut buf = *report; // ioctl mutates nothing, but the kernel wants a mutable slice
        let n = unsafe { libc::ioctl(self.fd.as_raw_fd(), hid_iocsfeature(buf.len()), buf.as_mut_ptr()) };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
}
```

### The encoder

```rust
// src/lighting/protocol.rs — pure
pub const REPORT_ID: u8 = 0x07;
pub const REPORT_LEN: usize = 960;

pub fn reports_for(state: &LightingState) -> Result<Vec<[u8; REPORT_LEN]>, EncodeError>;
```

Returns `C8`, `CB`, `CE` (or `C8`, `CB` for off). It refuses to truncate: payload
over 960 bytes, a zone id outside 1..=24, or brightness outside 1..=9 is an
error, not a clamp hidden in the UI.

### The worker

```rust
pub enum Command { Apply(LightingState), Off, Reapply, Rescan }
pub enum Event   { Applied { took: Duration }, NotFound, Denied(String), Lost }
```

One thread, one `Device`, one mutex-equivalent (a plain `&mut self` loop body).
Commands coalesce: while a `CB` is in flight, newer `Apply` commands collapse into
the latest state, so a slider drag produces one write per settled value, never a
queue of 200.

### The app wiring

Identical shape to SeeMyMac's `_poll`: `cx.spawn_in(window, async move |this, cx| loop { … })`,
selecting on `woken.recv()` and a timer, then `this.update_in(...)` → `cx.notify()`.

---

## 8. User interface

One `1280 × 800` window (minimum `720 × 560`), client-side header bar with a
segmented page switcher, in the SeeMyMac visual language: dark chrome, `#161B22`
surfaces, `#30363D` borders, `#2F6FEB` accent, 6 px radii, Inter when installed.

A `ui_design/` handoff is produced before `view.rs`, mirroring SeeMyMac's
workflow: `ui_design/README.md` with the tokens, one HTML mockup per page in
`ui_design/project/`, and `canvas.json`. The mockups are the spec the Rust view is
written against.

### Lighting

- **Mode row**: `Static` · `Off` · `Whole keyboard` (single colour). Nothing else
  in v1. Lenovo's 20-odd `animationId` effects are *not* reproduced, because
  animating means writing the controller at 15–20 Hz forever and the flash-wear
  risk is unresolved (see §10).
- **Zone canvas**: 24 rounded regions laid out as a keyboard, click to select,
  drag across to paint, shift-click to add. Selected regions get an accent ring;
  each shows its own fill colour at its own brightness. Implemented with
  `on_mouse_down` / `on_mouse_move` / `on_mouse_up` on a container div and
  per-zone bounds computed once per layout.
- **Colour**: swatch row of the last eight colours plus a custom HSV picker
  (hue strip + S/V square, both `div` + `linear_gradient`, handles driven by
  drag). No hex entry in v1; a hex field is a Phase 5 nicety and needs the
  hand-rolled text input.
- **Brightness**: a 1–9 stepped control drawn as nine segments, labelled with the
  firmware step so it matches Fn+Space. Percentage is secondary text.
- **Presets**: rows in a preferences-style list, saved to
  `~/.config/legionlight/presets/*.json`. Same schema as the live state.
- **Apply semantics**: every edit is applied on change, debounced 150 ms by the
  worker's coalescing. There is no "Apply" button — the keyboard *is* the preview.
- **Status line**: `Applied in 42 ms`, or `Read-only: 048d:c615 not found`, or
  `Denied: device changed since open`. Always the truth, never a spinner.

### Zone calibration (the honest part)

Lenovo's configs contain the 24-zone layout with **placeholder geometry** — all
24 zones report the same `x/y/w/h`. There is no usable picture of which physical
region each `led_id` covers. Rather than guess and ship a lie, the app has a
calibration dialog:

1. Pick a suggested map: `row-major`, `column-major`, `6 × 4 blocks`, `custom`.
2. Press **Identify**: exactly one zone is lit at 100 % white for 1.5 s, then the
   next, while the dialog shows the number and asks which region it appeared in.
3. The map is stored per model (`83Q7.json`), exportable and importable.

Until calibrated, the canvas is drawn row-major and the page shows a banner:
*"Zone order is unverified on this model — run Identify."*

### Performance

- Platform profile: three radio rows driven by `platform_profile_choices`, so a
  kernel that adds a choice gets it for free.
- Live readouts: CPU/GPU/NVMe temperature, fan RPM, AC/battery.
- Explicit note that custom fan curves are unavailable on this SKU.

### Battery & Input

- Conservation mode toggle, with the 60 %-cap explanation.
- Fn-lock toggle.
- Fan mode selector — only when the attribute exists.

### About / Diagnostics

- Controller identity, interface, report sizes, descriptor signature offset,
  firmware reply bytes, calibration state, worker state, last error.
- A **"Copy diagnostic report"** button — text only, no device writes. This is
  what makes bug reports possible from other people's laptops.
- The report is produced by `legion_light_ctl doctor`, so it can also be run in a
  terminal and pasted into an issue.

### Optional later

GPUI has no GNOME Shell integration, so no Quick Settings toggle. The equivalent
is a tray-less second window with `WindowOptions` sized as a popover, plus a
`.desktop` action for `legion_light --off`. Deferred to Phase 5.

---

## 9. Privilege model

Lighting writes need an fd on `/dev/hidraw0`; the four `sysfs` switches need root.
Two stages:

**Phase 2 — udev, no daemon.** Ship
`SUBSYSTEM=="hidraw", ATTRS{idVendor}=="048d", ATTRS{idProduct}=="c615", TAG+="uaccess"`,
so the logged-in seat gets the fd and the GUI writes the keyboard directly. This
covers the whole lighting feature with zero IPC. The machine currently has a
broader `/etc/udev/rules.d/60-legionaura.rules` granting `MODE="0666"` on the same
IDs; the package replaces it.

**Phase 3 — daemon for the rest.** `legion_lightd`, a system service, exposes
`io.gitlab.kestermckinney.LegionLight` on the system bus via `zbus`:
`SetPlatformProfile(s)`, `SetConservation(b)`, `SetFnLock(b)`, `SetFanMode(s)`,
`GetTelemetry() -> a{sv}`, plus `Apply(LightingState)` so lighting can move behind
the same door and the GUI can drop the udev rule. Polkit action
`io.gitlab.kestermckinney.LegionLight.admin`.

The daemon also exists for two things the GUI cannot do on its own: re-applying
lighting after resume (firmware resets the keyboard on S3), and owning the device
so two app instances cannot fight over it.

`legion_lightd` hardening: `ProtectSystem=strict`, `ProtectHome=yes`,
`DeviceAllow=/dev/hidraw0 rw`, `RestrictAddressFamilies=` empty, no network, no
exec. It links the same `lighting` and `sysfs` modules as the CLI, so there is one
implementation of every write.

---

## 10. Safety

The controller is a small MCU with an unknown flash budget. Every rule below
exists because of that.

- **Identity gate.** Write only if all of: bus `0x03`, VID `0x048D`,
  PID `0xC615` (allowlist, not a range), interface `00`, descriptor *contains*
  `06 89 FF 09 07 A1 01 85 07`, and feature report `0x07` exists with a 959-byte
  payload. Any mismatch → read-only mode with the reason shown in About.
- **No animation loop in v1.** Static frames only, applied on change. The
  reference project measured 37–47 ms per `CB` write (~17 fps ceiling) and
  deliberately does not animate for this reason.
- **Rate limiting.** ≥ 50 ms between reports; a hard cap of 5 s⁻¹ sustained.
  UI debouncing means a slider drag produces a handful of writes, not hundreds.
- **Bounded encoder.** Payload ≤ 960 bytes, `1 ≤ led_id ≤ 24`, group count ≤ 24,
  brightness 1..=9. The encoder returns `Err`, it never truncates.
- **Sequence atomicity.** `C8` → `CB` → `CE` is one `Command::Apply` and one
  worker iteration. A partial sequence leaves the previous frame intact, which is
  the safe failure.
- **Escape hatch — corrected by the first real write.** Fn+Space is handled in
  firmware and still works, but it is **not a colour reset**: it now cycles the
  profile this program stored. Whatever frame was last written is what Fn+Space
  turns on and off, so after `off` the hotkey has nothing to turn back on. The
  recovery path is therefore "write a colour again", not "press the hotkey" —
  which means the UI must always offer an obvious *on* control, and `off` must
  not be the persisted state on logout. See `docs/protocol-notes.md` §8.
- **Dry-run is the default.** `legion_light_ctl apply` and `... off` print a
  hexdump of every report and open nothing for writing; only `--write` sends.
  Every golden test runs through the printing path.
- **No firmware, no BIOS, no MUX, no DKMS.** Out of scope, permanently.

---

## 11. Phased plan

**Phase 0 — prove the wire (blocking, needs the laptop).** *Passed.*
`probe` (read-only), then `apply --all 20,20,20 --write`, `off --write`, a
single-zone write, all black, and `off` again. Success criteria: the writes are
accepted (they are — every ioctl returns 0, ~7 ms each), `dmesg` stays clean (no
entries at all since the first write), and the device survives (it does; `probe`
still reads the same descriptor). **Visible keyboard changes confirmed by the user. Suspend/resume is deferred.
Fn + Space currently toggles the static red frame; firmware-cycle restoration
remains unresolved.** Full numbers in
`docs/protocol-notes.md` §8.

**Phase 1 — core crate, still no GUI.** *Done.*
`model`, `lighting::protocol`, `lighting::descriptor`, `lighting::hid`, `sysfs`
and the full `legion_light_ctl`. Fifteen tests: encoder byte-for-byte goldens, the
real 231-byte `c615` descriptor as a fixture (signature at offset 23, so a prefix
check fails the build), the 327-byte LampArray descriptor as a negative case, and
truncated-descriptor fuzzing. `lighting::worker` and fake `/sys` trees move to
Phase 2, where they are first needed.

**Phase 2 — the GPUI app, static lighting only.** *In progress.*
`ui_design/` handoff, `main.rs` + `app.rs` + `view.rs` + `panels.rs`, Lighting
page, brightness, off, presets, About, `ui_tests.rs`. Direct hidraw access behind
the udev rule.

**Phase 3 — daemon and the other options.**
`legion_lightd` with `zbus`, polkit action, systemd unit, resume re-apply; then
Performance, Battery & Input pages. Retire the `0666` rule.

**Phase 4 — calibration and diagnostics.**
Identify flow, map import/export, `doctor` output wired into About.

**Phase 5 — polish.**
LampArray fallback backend, hex colour entry, popover window, AppStream
metadata, translations.

---

## 12. Packaging

Native RPM for Fedora. Flatpak is a poor fit: it cannot reach `hidraw` without
`--device=all`, and the sysfs switches need root regardless.

```
/usr/bin/legion_light
/usr/bin/legion_light_ctl
/usr/libexec/legion_lightd                     (Phase 3)
/usr/lib/systemd/system/legion_lightd.service  (Phase 3)
/usr/lib/udev/rules.d/71-legionlight.rules
/usr/share/polkit-1/actions/io.gitlab.kestermckinney.LegionLight.policy   (Phase 3)
/usr/share/applications/legionlight.desktop
/usr/share/icons/hicolor/.../legionlight.png
/usr/share/metainfo/io.gitlab.kestermckinney.LegionLight.metainfo.xml
```

`legionlight.desktop` follows SeeMyMac's file exactly:

```ini
[Desktop Entry]
Type=Application
Name=Legion Light
Comment=Keyboard backlight and performance control for Lenovo Legion laptops
Exec=legion_light
Icon=legionlight
Categories=System;Settings;HardwareSettings;
StartupWMClass=legionlight
```

Build: `cargo build --release` with the same release profile SeeMyMac uses
(`lto = true`, `strip = true`, `panic = "abort"`, `codegen-units = 1`). The
`[profile.dev.package."*"]` opt-level 3 override is worth keeping: GPUI layout in
an unoptimised dependency build is noticeably sluggish.

---

## 13. Testing

No Python, no shell scripts that matter, no manual-only steps.

- **Encoder golden tests** (`cargo test`, no hardware): fixed state → exact
  960-byte reports as byte slices. Includes 24 distinct colours, one uniform
  colour, off, and the descriptor-offset-23 case a prefix check would fail.
- **Identity tests**: fake `/sys` trees for correct device, wrong PID, wrong
  interface, missing report `0x07`, truncated descriptor, symlink escape.
- **Worker tests**: coalescing under a burst of `Apply`, rate limiter spacing,
  brightness rejection, partial-sequence recovery, `Lost` on device removal.
- **GPUI visual tests** (`#[gpui::test]` / `VisualTestContext`, as in
  `ui_tests.rs`): click a zone → it becomes selected and a `Command` is queued;
  drag the brightness control → one command per settled step; a `Denied` event
  renders the read-only banner. The worker is replaced by an in-process fake that
  records commands and never touches hardware.
- **Hardware smoke test** (manual, documented in `README.md`): probe, static
  colour, single zone, brightness sweep, off, suspend/resume, replug.
- `legion_light_ctl doctor` produces the text that goes in a bug report, so the
  smoke test is reproducible on someone else's machine.

---

## 14. Open questions

1. **Does `c615` accept the `c195` write sequence?** Same descriptor family, same
   report IDs, but unproven. Phase 0 answers this.
2. **What is the physical 24-zone layout?** Lenovo's configs have placeholder
   geometry. Calibration measured from real hardware must supply it.
3. **Is `0xCC` (32-byte feature report) a live legacy path on this device?** Reads
   fail with `EPIPE`. Not needed; not pursued.
4. **Is there a vendor animation command?** The `CB` group header contains
   `0x01 0x0B`, and Lenovo's profiles carry `animationId` 0–22. Tempting, but
   inventing a mapping would risk a bad write. Left alone in v1.
5. **Flash endurance.** Unknown. Mitigated by not animating.
6. **Does the firmware Fn+Space cycle fight our state?** Assume yes; re-apply on
   resume and offer a manual "Re-apply" in About.
7. **Repo name.** The directory is `LegionForGnome`, but the app is GPUI and no
   longer GNOME-specific. The crate and binaries here are `legion_light` /
   `legionlight`; renaming the directory is a separate decision.
