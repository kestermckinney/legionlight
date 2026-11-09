# Protocol notes and raw evidence

Everything here was read from local artifacts or the local machine. Nothing was
written to the controller. Treat all of it as *observation*, not specification.

Host: Lenovo Legion 5 15AHP11, DMI `83Q7` / `LNVNB161216`,
Fedora 45, kernel `7.2.15-300.fc45.x86_64`.

---

## 1. Installer contents

`legionspace-1.9.11.5_x64_rtm.exe`
`sha256 5a2f3b1bf7f591b6cdfe780f327b6f593ec52b39d291bf6da5a709f635e2859d`
592 MiB, PE32, `LegionSpaceInstall.exe`, FileVersion 1.9.11.5, 2026-02-06.

Sections: `.text`, `.rdata`, `.data`, `.fptable`, `.rsrc` (618,875,088 bytes),
`.reloc`. Payload is in `.rsrc`:

| Resource | Size | Format |
|---|---|---|
| `ZIPRES/108/1033` | 608.83 MB | 7-Zip archive, 5,050 files, 1.2 GB unpacked |
| `ZIPRES/107/1033` | 7.13 MB | ZIP, installer theme assets |
| `DLL/102/1033` | 1.48 MB | PE32 DLL |
| `DLL/105/1033` | 0.85 MB | PE32+ console |
| `DLL/128/1033` | 0.30 MB | PE32 DLL |

Marker strings: `Inno Setup` @ 577,213,931; `7-Zip` @ 619,933,488; `WiX` @
36,770,460. No `innoextract`/`cabextract` needed — the resource tree was walked
directly and the archives listed with `7z`.

### Lighting components (from `ZIPRES/108`)

```
se/LenovoLightingService.exe                         PE32+ x86-64, 6 sections
se/plugins/SELuminousNB/SELuminousNB64.dll           PE32+ x86-64, 7 sections
se/plugins/SELuminous/SELuminous64.dll               PE32+ x86-64
se/lgtplugins/SEBasicLighting{,Helper}/              legacy path + configs
se/lgtplugins/SELuminousDynamicEffect/               effect engine
se/SparsePackage/Lightingpackage.msix                sparse MSIX
se/SparsePackage/Lightingpackage2503.msix            variant
```

Neither the service nor the plugin is .NET (no `mscoree.dll` import, no `BSJB`
signature). They are native C++/ATL with RTTI intact.

### `SELuminousNB64.dll` imports

`gdiplus, SETUPAPI, WINUSB, KERNEL32, USER32, ADVAPI32, ole32, SHELL32, OLEAUT32,
SHLWAPI, GDI32, POWRPROF, HID, WTSAPI32`

Relevant symbols:

```
WinUsb_WritePipe, WinUsb_ReadPipe, WinUsb_ControlTransfer, WinUsb_Initialize,
WinUsb_QueryPipe, WinUsb_QueryInterfaceSettings, WinUsb_SetPipePolicy,
WinUsb_FlushPipe, WinUsb_AbortPipe, WinUsb_ResetPipe, WinUsb_GetOverlappedResult
HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData, HidD_FreePreparsedData,
HidP_GetCaps
SetupDiGetClassDevsW/A, SetupDiGetDeviceInterfaceDetailA, SetupDiDestroyDeviceInfoList
```

Device filter strings found in the module: `USB\VID_048D&PID_5000`,
`USB\VID_17EF&PID_F006`.

### `LenovoLightingService.exe` strings

```
Lighting Driver Service            (service display name)
Software\Lenovo\LightingDriver     (registry key)
WB_Mutex_LightingDriver            (single-instance mutex)
LenovoLightingService.log
[smain] main LightingDriver.exe start to run, params:[%s]
...\framework\src\Modules\LightingService\LightingDriver.cpp
...\framework\src\Modules\LightingService\ClientForSe.cpp
...\framework\src\Modules\LightingService\LightingStatusManager.cpp
ConnectNamedPipe / CreateNamedPipeW / DisconnectNamedPipe / GetNamedPipeClientProcessId
```

RTTI: `LightingDriver::CSessionModule`, `LightingDriver::LightingStatusManager`,
`LPCModule::CPipelineClient`, `LightingDriver::CServiceModule`.

Architecture:

```
LegionSpace.exe (CEF)
  → LenovoLightingService.exe (ATL service, named pipe, single mutex)
    → SELuminousNB64.dll (plugin, contract "SELuminousNB", moduleid 2015)
      → WinUSB pipes on the ITE 8258
```

### MSIX (`Lightingpackage.msix`)

```
Identity Name="LegionLightingController" ProcessorArchitecture="x64"
Publisher "CN=Lenovo, O=Lenovo, L=Morrisville, S=North Carolina, C=US"
Version 1.0.0.0
uap10:AllowExternalContent = true
Capabilities: runFullTrust, unvirtualizedResources
```

This is the Windows 11 Dynamic Lighting registration — proof that Lenovo
considers the open LampArray path shippable.

### Config model (`NBLightingConfig/`)

```
brightness.json                    {"brightness": 7}
default_profile_Y24zone.json       profiles[].layers[].animationConfig
keyboard_24zone.json               Layout.Leds[24], tags 1..24, host 0
subdevice.json                     Layout.Leds[216], per-key, hosts 0..215
light_us.json                      mapid[131] — key name → tag/host/hidcode
outdevices.json                    external mouse/keyboard/monitor descriptors
```

Example animation config (`default_profile_Y24zone.json`, profile 2):

```json
{"clockwise": 0, "colorList": [{"r": 21, "g": 141, "b": 221}],
 "colorSize": 1, "colorType": 1, "speed": 2,
 "animationId": 21,
 "keys": [1..24], "layerId": 1, "isApply": true}
```

`animationId` values observed across the shipped profiles: `0, 1, 2, 4, 6, 7, 8,
9, 10, 11, 20, 21, 22`.

`keyboard_24zone.json` gives all 24 zones identical geometry
(`x=-166.0, y=-60.0, w=85.0, h=105.0`), i.e. a placeholder. **There is no usable
zone→region map in the installer.**

`light_us.json` is the abstract per-key map (131 entries, `tag` = key id, not the
24-zone id). `subdevice.json` is the 216-lamp per-key model. Neither yields the
24-zone grouping.

---

## 2. Local device

```
Bus 001 Device 003: ID 048d:c615 Integrated Technology Express, Inc. ITE Device(8258)
```

```
/sys/class/hidraw/hidraw0 → .../1-4/1-4:1.0/0003:048D:C615.0001/hidraw/hidraw0
/sys/class/hidraw/hidraw1 → .../1-4/1-4:1.1/0003:048D:C615.0002/hidraw/hidraw1
```

USB descriptors: 2 interfaces, both class 3 (HID), one interrupt IN endpoint each
(`0x81`, `0x82`), `wMaxPacketSize` 64.

`/lib/modules/7.2.15-300.fc45.x86_64/kernel/drivers/hid/hid-ite.ko.xz` does not
claim `048d:c615` (its aliases are `048d:8595` and others); both interfaces bind
to `hid-generic`. No kernel driver participates in lighting on this machine.

### Report descriptors

`hidraw0` (231 bytes) parsed:

```
feature:  id=0x07  7672 bits (959 B)
          id=0x5A 128 bits (16 B)
          id=0xCC 256 bits (32 B)
input:    id=0x01 336 bits, id=0x02 16, id=0x03 8, id=0x05 72
output:   id=0x01 8 bits
```

`hidraw1` parsed:

```
feature: id=0x01 176 bits (22 B)   LampArrayAttributes
         id=0x02 16  bits (2 B)    LampAttributesRequest
         id=0x03 224 bits (28 B)   LampAttributesResponse
         id=0x04 400 bits (50 B)   LampMultiUpdate
         id=0x05 72  bits (9 B)    LampRangeUpdate
         id=0x06 8   bits (1 B)    LampArrayControl
```

The six feature reports are exactly the LampArray set; there are no input/output
reports, so LampArray control is feature-report only.

### Descriptor head (hidraw0)

```
00000000: 0689 ff09 10a1 0185 5a09 0115 0026 ff00
00000010: 7508 9510 b100 c006 89ff 0907 a101 8507
00000020: 0901 1500 26ff 0075 0896 bf03 b100 c006
00000030: 89ff 09cc a101 85cc 0901 1500 26ff 0075
00000040: 0895 20b1 00c0 0501 0906 a101 8501 7501
...
```

Signature `06 89 FF 09 07 A1 01 85 07` found at **offset 23**, not at offset 0.
Collection order is: usage `0x10` / report `0x5A`, then usage `0x07` / report
`0x07`, then usage `0xCC` / report `0xCC`.

**Consequence:** a prefix check rejects this device. Use a substring check.

---

## 3. Read-only probes

`HIDIOCGFEATURE` = `(3 << 30) | (size << 16) | ('H' << 8) | 0x07`.

```
report 0x5A (16 B):  00 ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff
report 0xCC (32 B):  EPIPE  (Errno 32) — reads not accepted
report 0x07 (959 B): 07 03 09 00 01 00 55 00  then zeros
                     nonzero bytes: index 0=0x07, 1=0x03, 2=0x09, 4=0x01, 6=0x55
```

`0x07` read is a status/version reply. It does **not** echo the current colour
buffer.

### LampArray probes

`GET_FEATURE 0x01` (23-byte buffer; 22 is rejected with `EINVAL`):

```
01 | 01 00 | c8 14 05 00 | d8 d0 01 00 | b8 0b 00 00 | 01 00 00 00 | 60 09 00 00
     count   width 333mm   height 119mm  depth 3mm     kind=1 kbd    interval 2400us
```

`SET_FEATURE 0x02` with lamp id, then `GET_FEATURE 0x03` (29 bytes), identical for
lamp 0, 1 and 23:

```
03 | 00 00 | b8 4a 02 00 | b8 82 01 00 | e8 03 00 00 | a0 0f 00 00 | 01 00 00 00
     id     x 150.2mm     y 98.5mm       z 1.0mm       4000us         purpose=1
   | ff | ff | ff | 01 | 01 | 00
     R    G    B    int  prog bind
```

One lamp, 255 RGB levels, one intensity level, programmable, purpose `Control`.
Therefore LampArray can set one uniform colour, nothing more.

---

## 4. The 24-zone report

Source: `ulrickpsp/legion-control-linux` (MIT), `legion_control/rgb.py` and
`docs/RGB-PROTOCOL.md`. Physically validated by that project on `048d:c195`
(83LU, Ubuntu 26.04, Linux 7.0, BIOS Q6CN79WW). Our descriptor family matches.

That reference is Python. It is used as **evidence only** — the byte layout and
the constants below are the deliverable; LegionForGnome re-derives the encoder in
Rust and reuses no code from it. One behavioural difference is intentional: it
prefix-checks the descriptor, we substring-match (see §2).

Constants:

```
report id        0x07
buffer           960 bytes (959 payload + report ID)
zone count       24
profile id       0x01
brightness       1..9
inter-report     10 ms (reference); 37-47 ms observed per CB write
```

Envelope:

```
[0] = 0x07
[1] = command
[2:4] = payload length, LE u16
[4:] = payload
```

### Report: select profile

```
07 C8 01 00 | 01
```

### Report: static zones

```
07 CB <len le16> | 01 01 01 | <group>...
```

where `<len>` is the total payload length (offset - 4 in the reference
implementation).

### Group encoding

Per distinct colour, with `group_index` starting at 1:

```
group_index, 0x06, 0x01, 0x0B,
0x02, 0x02, 0x03, 0x00, 0x04, 0x00, 0x05, 0x02, 0x06, 0x00,
0x01, R, G, B,
count,
led_id LE u16 , ...  (count entries)
```

`led_id` is 1-based, range 1..24. Zones are grouped by colour, so a uniform frame
is a single group and a 24-colour frame is 24 groups.

The `0x02..0x06` pairs and `0x01 0x0B` are opaque. They are reproduced verbatim.

### Report: brightness

```
07 CE 01 00 | <n>       n = clamp(1, 9, round(percent / 100 * 9))
```

### Off sequence

```
07 C8 01 00 | 01
07 CB 03 00 | 01 01 01
```

### Ordering

`C8`, `CB`, `CE` in that order, 10 ms apart. `CB` is the expensive one
(37–47 ms measured), so the practical ceiling is ~17 frames/s.

### Legacy 4-zone path

`Nivedck/LegionAura` uses `0xCC 0x16` over a USB control transfer for older
4-zone keyboards. This device exposes `0xCC` as a *feature* report on interface
00 and rejects feature reads with `EPIPE`. That path is not applicable and was
not pursued.

---

## 5. Other machine interfaces

```
/sys/firmware/acpi/platform_profile          = performance
/sys/firmware/acpi/platform_profile_choices  = low-power balanced performance
/sys/bus/platform/devices/VPC2004:00/fn_lock           = 0        (0644 root)
/sys/bus/platform/devices/VPC2004:00/fan_mode          = 0        (0644 root)
/sys/bus/platform/devices/VPC2004:00/conservation_mode = 1        (0644 root)
```

hwmon:

```
hwmon0 ACAD                    hwmon5/6 ucsi_source_psy_USBC*
hwmon1 acpi_fan (fan1_input)   hwmon7 amdgpu
hwmon2 acpitz                  hwmon8 k10temp  (temp1 = 47.25 C)
hwmon3 BAT1                    hwmon9 yogafan (fan1 = 2000 rpm)
hwmon4 nvme
```

WMI:

```
lenovo_wmi_gamezone  bound: 887B54E3-DDDC-4B2C-8B88-68A26A8835D0-23
                     exposes only `platform-profile`
lenovo_wmi_other     bound: DC2A8805-3A8C-41BA-A6F7-092E0089CD3B-27
                     publishes no fan-target / ppt_* attributes on this SKU
lenovo_wmi_capdata, lenovo_wmi_events, lenovo_wmi_hotkey_utilities also bound
```

No `/sys/class/leds/*::kbd_backlight`, no `legion` platform device, no
`charge_control_end_threshold`.

### Existing udev rule on this machine

`/etc/udev/rules.d/60-legionaura.rules` already contains:

```
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="048d", ATTRS{idProduct}=="c615", MODE="0666"
```

That is why `/dev/hidraw0` and `/dev/hidraw1` are `crw-rw-rw-`. It came from an
earlier LegionAura experiment and should be replaced by a `TAG+="uaccess"` rule
when the daemon lands.

---

## 6. Reference projects consulted

| Project | Licence | Relevance |
|---|---|---|
| `ulrickpsp/legion-control-linux` | MIT | the 24-zone `0x07` protocol, validated on `048d:c195` |
| `Nivedck/LegionAura` | — | legacy 4-zone `0xCC 0x16` path (not applicable here) |
| `4JX/L5P-Keyboard-RGB` | — | 2025-model tracking issue #234 (`c195`, 24 zones, Dynamic Lighting) |
| `Dhruvgoel3829/LOQ-24-zone-RGB-controller` | — | 24-zone USB captures, effect presets |
| `xz-dev/hid-rgb-ctl` | — | LampArray report layouts, verified against MS reference |

Nothing from these projects was copied into this repository; the report encoder is
re-derived from observed bytes and documented above.

---

## 7. What is *not* known

- ~~Whether `048d:c615` accepts the `c195` write sequence.~~ **Yes — see §8.**
  The writes are accepted with no error, and no kernel-side fallout.
- The physical mapping of logical zones 1..24 to keyboard regions.
- Whether the opaque `CB` group header encodes an effect id, and whether
  `animationId` 0–22 map to `CB` opcodes.
- Flash endurance of the ITE 8258.
- Whether the firmware Fn+Space cycle overwrites host-applied state.
- Whether `0xCC` accepts writes on this device at all.

---

## 8. First write to `048d:c615`

The `c195`-shaped sequence was sent to interface 00 on a Legion 5 15AHP11. Nothing
about the bytes was changed from §4; the only difference is the destination PID.

Environment: `/dev/hidraw0` opened `O_RDWR|O_NOFOLLOW|O_CLOEXEC`, gate re-checked
before writing (bus `0003`, `048d:c615`, signature at 23, report `0x07` = 959 B),
reports spaced 50 ms apart.

### Sequence 1 — uniform warm grey, brightness 7

```
$ legion_light_ctl apply --all 20,20,20 --write
  [1] C8 select profile    sent in  6.65 ms   07 c8 01 00 01
  [2] CB zone table        sent in  6.92 ms   07 cb 46 00 01 01 01 | group 1, 24 LEDs
  [3] CE brightness        sent in  6.93 ms   07 ce 01 00 07
  done in 120.70 ms
```

### Sequence 2 — off

```
$ legion_light_ctl off --write
  [1] C8 select profile    sent in  6.99 ms
  [2] CB blank table       sent in  6.97 ms   07 cb 03 00 01 01 01
  done in 64.06 ms
```

### Sequence 3 — one zone, full brightness

```
$ legion_light_ctl apply --zone 1=255,255,255 --brightness 9 --write
  [1] C8 select profile    sent in  7.16 ms
  [2] CB zone table        sent in  7.95 ms   group 1 = LED 1 white, group 2 = LEDs 2..24 black
  [3] CE brightness        sent in  6.88 ms   07 ce 01 00 09
  done in 122.19 ms
```

### Sequence 4 — all black, then off again

Both accepted; timings identical to the runs above.

### What the controller did

Every `HIDIOCSFEATURE` returned 0. The `0x07` status reply did change across the
writes, but **it is not a picture of the framebuffer** and it is not stable enough
to use as read-back:

| when | `07 03 09 00 01 00` + … |
| --- | --- |
| before any write | `55 00` |
| after sequence 1, immediately | `8b 00 1b 00` |
| after sequence 2 (off) | `63 c6 00` |
| a second later, no writes in between | `5d 00 41 00`, then `64 00 46 00` |
| after sequences 3–4 | `00 00 00` |

Those trailing bytes drift without a write and settle at zero after the final
`off`, so treat them as opaque vendor telemetry (the §3 note stands: `0x07` does
not echo colour). **There is no read-back path**: the only way to know the frame
landed is to look at the keyboard, which is exactly why the UI must treat a
successful `write()` as "sent", not "confirmed".

### Kernel side

`journalctl -k` since the first write: no entries at all — no USB reset, no
`hidraw` complaint, no `-EPIPE`, no timeout. `probe` afterwards still opens the
node, still sees the same descriptor and the same three feature reports.

### Cost per write

Each report costs ~7 ms of ioctl time plus 50 ms of deliberate spacing, so a
static frame is ~120 ms and `off` is ~64 ms. That is the number the Phase 2 rate
limiter must respect; it also rules out animations, as `DESIGN.md` §10 says.

### Side effects the reference protocol does not mention

The writes did not just paint a frame; they **replaced the stored profile** that
the firmware hotkey replays. Observed on the machine after the runs above:

- The keyboard showed the colour that was last written (red, in this run).
- **Fn+Space stopped cycling colour and brightness.** It now just turns that one
  stored colour on and off. The firmware's own default look does not come back.
- Writing a new frame immediately becomes the thing Fn+Space toggles again.

So `C8`/`CB`/`CE` is a *persistent configuration write*, not a transient paint.
That inverts the "Fn+Space is the escape hatch" assumption in `DESIGN.md` §10:
the hotkey is a power switch for whatever we stored, not a way back to Lenovo's
default. Two consequences for the application:

1. `off` must never be the persisted state — a blank table leaves Fn+Space with
   nothing to turn on, which reads as a broken backlight.
2. Every session that writes has to leave a colour the user chose, and the UI
   needs a visible "on" control before it offers "off".

Still unknown, and only a power cycle can answer it: whether the stored profile
lives in the 8258's flash (survives reboot and power loss) or only in RAM. The
~7 ms write time on `c615` is suspiciously short for a flash commit, so RAM-only
is plausible; either way the app must re-apply at startup and after resume rather
than assume the controller remembered.

### Colour control is not verified (November 8 follow-up)

The user reports that both GUI swatches and a direct CLI uniform-blue write
leave the physical keyboard unchanged. The GUI preview changes correctly.
Successful HID ioctls do **not** establish that `c615` implements the `c195`
reference protocol. Earlier claims of colour-control success are too strong.

A bounded experiment sent the same blue frame with `C0 03` in header bytes 2–3,
following alstergee/legion-spectrum-control's Spectrum framing. Each write
returned 960; GET_FEATURE after each returned the same prefix
`07 03 09 00 01 00 00 00`. Its meaning is unknown. Visible result pending.
The production encoder is unchanged pending evidence.

Follow-up: Fn+Space cycles two dark settings and one red setting (user report).
`CA` query returned `07 ca 01 00 02`: current profile 2. `CD` returned brightness
9. `CC` with no payload returned the current layer, including red at offsets
22..25. This disproves the earlier blanket claim that no read-back path exists.
`CC` queries carrying 1/2/3 all returned profile 2; those payloads do not select
which profile to read on this controller.

Saved the original layer to `/tmp/legion-profile2-before.bin`. Changed only the
opcode CC→CB and RGB red→blue, preserving the complete returned report and its
profile 2 and zone data. Subsequent CC read-back contained blue. Visible result
pending. The layer's key count is 28 (0x1c), not our assumed 24; its returned key
data needs further decoding before replacing the production encoder.

Reselecting profile 2 did not visibly change red (user confirmed). Profile
read-back is not proof of displayed output. Read-only LampArray discovery on
hidraw1 returned LampCount=1; lamp 0 attributes report RGB level counts 255 and
IntensityLevelCount=1. This contradicts the app's assumed 24 controllable zones.
Sent one standard LampArray test: autonomous mode off (`06 00`), range update
`05 01 00 00 00 00 00 00 ff 01` (lamp 0, blue, intensity 1). ioctl returned 10.
Visible confirmation pending; no production backend change yet.

The user confirmed that the standard LampArray blue/intensity-1 test also left
the keyboard red. Restored LampArray autonomous mode with `06 01` after testing.
Neither backend has demonstrated visible colour control on this exact device.
Further writes should be based on a known-working c615 implementation or a USB
capture from Lenovo's working Windows software, rather than additional guessed
packets. Firmware-reported topology and stored layer reads alone do not prove
that either interface controls the physical backlight.

User subsequently reported Fn+Space could show blue but no longer red, after
LampArray autonomous mode was restored. Thus the standard-interface test may
have taken effect on release to firmware or a later hotkey transition; immediate
visual observation was insufficient. This does not establish independent colour
slots or prove profile activation semantics. Sent the same bounded LampArray
sequence with red to restore the prior colour: `06 00`,
`05 01 00 00 00 00 ff 00 00 01`, `06 01` (50 ms spacing).
Visible restoration pending. GUI backend remains unchanged.

The red restoration sequence did not visibly restore red: the user reports only
blue and dark while cycling Fn+Space. Do not describe this sequence as a working
restore or the standard backend as verified. Blue became reachable sometime
after the experiments, but attribution and activation timing remain unresolved.
Stop further speculative writes; obtain a known-working exact-model trace.
