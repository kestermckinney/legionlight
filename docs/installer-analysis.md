# Legion Space installer investigation — November 8, 2026

I inspected the local `Downloads/legionspace-1.9.11.5_x64_rtm.exe` without
executing it. The initial investigation was static; the follow-up below records
bounded Linux hardware tests. This is not a verified recipe for changing the
physical lights.

The Python scripts and the C kernel diagnostic this record mentions were one-off
investigation tools. They were removed once the Rust app implemented the
protocol; they remain in the Git history (commit `bb76310`).

## The newer component matters

The installer contains both the older `se/plugins/SELuminousNB/SELuminousNB64.dll`
and `se/lgtplugins/SEBasicLighting/SEBasicLighting64.dll`. The newer component
contains an explicit `0xc615` entry at virtual address `0x18005ce30`.
Its associated four-integer table entry is `(3, 4, 4, 1)`, copied from
`0x180134fb0`. The meanings of those fields still need tracing; the device ID
alone does not establish the selected class or the activation sequence.

The basic lighting DLL SHA-256 is:
`808655dfee2648816fc997bc5b7590a97df123f91d4f86d4a465cf07c5841ac9`.
Addresses below assume its preferred image base, `0x180000000`.

## Control commands missing from the current investigation

The basic lighting component constructs these vendor feature reports and passes
960-byte buffers to its transport through virtual method offset `0xf8`.

| Evidence | Prefix / behavior | Limits |
| --- | --- | --- |
| `0x1800a6ab0`, construction at `0x1800a6b7c` | `07 AA 02 00 (bOn XOR 1) 00`, then zero padding | Logged as `SwitchDynamicMode`; mapping to the exact device remains to be established. |
| `0x1800a115a`–`0x1800a117c` | `07 D0 02 00 opcode profile`, then zero padding | `opcode` is `04` when the function's boolean argument is true, otherwise `DC`. |
| Branch at `0x1800a1107` | `07 D0 02 00 DC profile`, then zero padding | A separate policy branch forces firmware control regardless of the argument. |

The D0 branches log firmware control and firmware/app mode respectively.
The profile byte comes from the first byte of a referenced profile object.
The transport's fourth argument is `0x96`; its meaning is not established here.

A higher-level dynamic-mode wrapper at `0x180056e00` logs the inverse of its
boolean argument with the labels `0: Dynamic Lighting` and `1: KB Autonomous`,
then dispatches through virtual offset `0x78`. This is supporting evidence for
mode ownership, but I have not proved that this dispatch resolves to the AA
implementation for c615.

These commands are a concrete explanation to investigate for the difference
between storing a colour and displaying it. They are not yet proof of the cause.
In particular, the prior standard LampArray `06` tests did not exercise these
vendor `AA`/`D0` commands.

## What remains before implementation

- Trace c615's table entry through device construction and virtual methods.
- Recover the call order around mode selection, profile write, brightness, and
  profile activation, including policy branches for power and quiet mode.
- Decode the exact c615 colour/effect payload and key identifiers; do not assume
  that the older 24-zone encoder applies unchanged.
- Verify a bounded sequence against the physical keyboard before changing the
  production backend or claiming success.

## Reproduction and retained evidence

I extracted only the relevant plugin directories with 7-Zip:

```bash
7z x ~/Downloads/legionspace-1.9.11.5_x64_rtm.exe \
  -o/tmp/legion-installer-analysis \
  'se/plugins/SELuminousNB/*' \
  'se/lgtplugins/SEBasicLighting/*' \
  'se/lgtplugins/SEBasicLightingHelper/*' -y
objdump -d -Mintel \
  /tmp/legion-installer-analysis/se/lgtplugins/SEBasicLighting/SEBasicLighting64.dll
```

Selected disassembly is retained in
[basic-lighting-excerpts.txt](installer-evidence/basic-lighting-excerpts.txt).
The full temporary disassembly is `/tmp/legion-basic-disasm.txt`.

REA opened the DLL, but deep analysis was unavailable because no supported
decompiler was configured. `rea doctor` confirmed the missing engines. I used
`objdump`, PE section mapping, and UTF-16 string references instead, and closed
the REA session afterward. No installer code was executed, and no dependencies
or REA configuration were changed.

## Follow-up: virtual methods and live-colour format

I followed the MSVC run-time type information (RTTI) to establish the concrete
classes, rather than relying on nearby strings:

| Class / vtable | Method offset | Implementation | Behavior |
| --- | --- | --- | --- |
| `Device_24zone`, `0x1801303d8` | `0x70` | `0x1800a1000` | Policy-controlled D0 firmware/app command |
| `Device_24zone` | `0x78` | `0x18009dd30` | AA control ownership |
| `Device_24zone` | `0x98` | `0x1800a12e0` | Build and send A1, then A8 |
| `Device_24zone` | `0xa0` | `0x1800a2750` | Save CB profile layers |
| `Device_24zone` | `0xd8` | `0x18009a900` | D0 with caller-supplied opcode/profile |
| `Device_24zone` | `0xf8` | `0x1800a0d20` | Validate payload length, dispatch transport |
| `Device_OneZone`, `0x180130708` | `0x78` | `0x18009dd30` | Same AA implementation |
| `Device_OneZone` | `0x98` | `0x1800a42f0` | Separate live-colour implementation |
| `Device_OneZone` | `0xd8` | `0x18009a900` | Same D0 builder |

The higher-level AA wrapper at `0x180056e00` passes its argument unchanged to
offset `0x78`. Its diagnostic logs the inverse with explicit labels. Thus wire
`07 AA 02 00 01 00` requests keyboard autonomy; `... 00 00` requests Dynamic
Lighting. This is stronger evidence than the earlier untraced AA implementation.

The live-colour serializer at `0x1800a21f0` emits:

- A1: `07 A1 <payload length LE16>`, then `(ID u8, R, G, B)` entries.
- A8: `07 A8 <payload length LE16>`, then `(ID LE16, R, G, B)` entries.
- Both buffers are 960 bytes with zero padding. The colour source is unpacked
  from `0xRRGGBB` into R, G, B order.
- A8 accepts IDs 501–577 and 1001–1062. It additionally accepts 1501–1528
  when the engine flag at offset `0x236` is set; that flag defaults to true.
  These are serializer branches, not proof of c615's physical lamp mapping.
- The caller sends A1 followed by A8 with transport argument 5. Profile/control
  callers use 150. At `0x180082944`, that argument reaches Windows `Sleep`,
  establishing 5 ms and 150 ms post-write waits.

Read operations at `0x180082a30` first send the query through the feature-write
function, then fetch the reply through the feature-read function and wait 50 ms.
Initializing a Linux GET_FEATURE buffer with a command does not select a query.

The offline packet generator reconstructs these
formats. It never opens a device. Checked fixed prefixes, little-endian IDs,
payload lengths, 960-byte size, and zero padding. Selected source disassembly is
in [control-and-live-colour.txt](installer-evidence/control-and-live-colour.txt).

## Linux hardware results

The user confirmed Windows is unavailable. All tests targeted the existing
048d:c615 interface after checking its sysfs identity.

Query results:

| Query | Result |
| --- | --- |
| `07 04 01 00` (shipped chip query) | `07 04 01 00 00`; meaning of zero unresolved |
| `07 03 BC 03` (shipped current-colour query) | Nine-byte `07 03 09 00 01 00 00 00 FF`; not the requested colour map |
| CA | Current profile 2 |
| CD | Brightness 9 |
| CC | 960-byte layer with blue RGB; same malformed/unclear 28-count key data as before |

Two bounded tests followed, without writing CB or replacing a stored profile:

1. Sent `07 D0 02 00 DC 02`, waited 150 ms, then
   `07 AA 02 00 01 00`, waited 150 ms. The user checked Fn+Space and reported
   **no change; still blue/dark**.
2. Sent AA with wire value 0, D0 opcode 04/profile 2, an empty A1 frame, then
   A8 red entries for the shipped serializer's 1501–1528 range. Used the recovered
   150 ms control waits and 5 ms live-frame waits. The user checked before pressing
   Fn+Space and reported **still blue or dark**.

After the second test, sent D0 DC/profile 2 and AA wire value 1 again. Every
SET_FEATURE returned 960; this proves transport acceptance only. CC readback
afterward matched the pre-test stored layer byte-for-byte. Firmware-control
commands were sent successfully; visible restoration of ownership is not proved.

The initial snapshot's statement that no HID writes were performed applies only
to its static phase. The follow-up sent query, ownership, and live-colour reports
as explicitly recorded above. It did not change the production Rust backend.

At this stage, exact-model backend selection and mapping remained unresolved.
Neither D0/AA activation alone nor this A8 range should be advertised as a fix.
The next investigation resolved backend selection, as recorded below.

## Firmware-confirmed OneZone backend

I downloaded Ghidra 12.2 from its official release and verified its SHA-256
(`2c2cd776db3c1f6a097ba418922378ac3649cadd5e844a6f5f088ae628a7be67`). A temporary
Temurin JDK enabled headless decompilation. These tools and the analysis database
are under `/tmp`; the REA registration and system Java installation are unchanged.

The decompiled device selector at `0x18003e760` reads `LENOVO_LIGHTING_DATA`,
selects lighting ID 0 (keyboard), and interprets `Lighting_Type` as follows:

| Condition | Backend |
| --- | --- |
| Bit 0 clear | Single-colour keyboard |
| Bit 0 set; `(Lighting_Type >> 1) & 7 == 1` | 4zone |
| Bit 0 set; type field 2 | Perkey |
| Bit 0 set; type field 3 | 24zone |
| Bit 0 set; type field 4 | OneZone |

The user exported the laptop's ACPI tables and WMI definitions using the
read-only export script.
`DSDT` defines the lighting data block as GUID
`4DD5BD84-15A9-47E2-AD65-CC61A5C62FD0`, object `A8`, two instances. Query instance
zero runs `WQA8(0)` → `GLTT(0)`, selecting its table from the live `KB01`/`KB24`
firmware flags.

The user ran read-lighting-type.py.
It temporarily bound the previously unclaimed data block to the stock
`wmi-bmof` driver, whose probe calls only `wmidev_query_block(wdev, 0, ...)` and
exports the cached buffer. I checked this in the installed module's disassembly
and the [kernel driver source](https://raw.githubusercontent.com/torvalds/linux/master/drivers/platform/x86/wmi-bmof.c).
The script removed its temporary binding and cleared its override afterward;
both cleanup conditions were independently checked.

**Live result: `00 00 09 03 00 03 01`, lighting type 9, RGB enabled, OneZone.**
The result is retained in [onezone-confirmation.json](installer-evidence/onezone-confirmation.json).
This agrees with the standard LampArray reporting one lamp. The old assumption
that this laptop has 24 controllable zones is disproved.

The selector instantiates `Device_OneZone` (factory enum 14). Its layout selector
at `0x180086a80` uses `keyboard_onezone.json`, containing a single nonvirtual
LED with tag 1. The loader `0x180086c70` and LED constructor preserve that tag as
the live packet ID. `Device_OneZone::` live serializer at `0x1800a5200` sends
tag 1 through A1, rather than the extended A8 range tested earlier.

A bounded AA0 → D0 04/profile 2 → `07 A1 04 00 01 FF 00 00` → empty A8 test
still produced **blue/dark**, according to the user. D0 DC/profile 2 and AA1
were sent afterward; the stored CC layer remained byte-for-byte unchanged.
That is a negative test of the confirmed OneZone live packet, not evidence
that the firmware is 24-zone or that A1 visibly works.

## Separate ACPI ownership gate

The installer also uses a control outside USB. `DynamicLightingWmiCtrlHelper`
at `0x180089950` gets feature `0x06010000` and, when necessary, sets it to 1.
The laptop's DSDT confirms that `LENOVO_OTHER_METHOD`, GUID
`DC2A8805-3A8C-41BA-A6F7-092E0089CD3B`, method `0x11` gets that feature by
returning `LIGC`. Method `0x12`, with an eight-byte LE feature/value input,
accepts 0 or 1, assigns `CUST`, calls `CSMI(0xC5, 0x05)`, and updates `LIGC`.
Thus USB AA/D0 commands do not exercise the entire Lenovo control path.
Selected AML is retained in [firmware-lighting-excerpts.txt](installer-evidence/firmware-lighting-excerpts.txt).

The existing Linux driver publishes no userspace control for this feature.
The exact-model diagnostic module
builds against the running kernel. Loading it queries only; an explicit root
write is required to change ownership. It allows only this feature on DMI
product 83Q7 with the confirmed OneZone descriptor, and attempts to restore the
original owner on unload after a change. The wrapper uses the machine's existing
enrolled akmods signing certificate; it neither changes Secure Boot nor enrolls
another key. Runtime and visual verification are still required.

### Ownership experiment and channel correction

The user ran the signed diagnostic: ownership read 0, changed to 1, and
the live-red test remained **blue**. The probe unloaded afterward. Result:
`/tmp/legion-owner-result-ry76ejqn.json`. That test used AA0.

A further source check found a mismatch in that sequence. Profile start at
`0x18006d4d4` passes argument 0 to wrapper `0x180056e00`; the wrapper passes
it unchanged through virtual slot 0x78. The OneZone implementation
`0x18009dd30` XORs the argument with 1, producing **AA wire 1**. Therefore
Lenovo's profile-start path selects AA1, even when the subsequent D0 control
selects application rendering. The previous AA0 tests do not rule out that
combination. The diagnostic now uses AA1 before D0 04/profile 2 and A1 tag 1
red, retaining the same restoration and no saved-profile writes. This corrected
sequence still needs visual verification.

The corrected AA1 ownership test also remained **blue**, with ownership 0 → 1
and successful probe unload. Result: `/tmp/legion-owner-result-dznps6qi.json`.
Neither tested AA channel has demonstrated visible OneZone A1 control.

### Recovered static OneZone profile

Decompiled save function `0x1800a5760` builds CB with prefix profile/1/1 and
appends groups from `0x1800a59a0`. Groups contain ID, property count 6, the
properties from `0x1800a6220`, key count, then LE16 key IDs. The installer
`default_profile_onezone.json` specifies static animation 11 on key 1.
The animation encoder defaults unsupported speed/clockwise/direction to 0/0/4;
fixed colour uses property 5 = 2 and one RGB triple. The transport
`0x1800a3db0` clamps payload lengths above 956 to 956.

A fresh read still shows profile 2, brightness 9, blue, and key count 28 with
only IDs 1/2/3 nonzero. This differs from the shipped single-key OneZone layout.
The bounded static-profile diagnostic
backs up this exact CC reply before testing a source-derived static red group
on ID 1. It restores the prior profile and brightness in finally, checks the
restored CC reply byte-for-byte, and retains all reports in a temporary directory.
Offline packet checks passed; this new static test has not yet been run.

The first static-test invocation stopped at its precondition before any profile
write (`/tmp/legion-static-onezone-8k1bj300`). A subsequent live query returned
CA profile **0**, CD brightness 9, and the unchanged blue CC profile **2**.
Thus the selected profile and returned stored profile can differ; the test's
old error message did not explain that distinction. The diagnostic now records
initial replies before validating and, if CA is not 2, asks the user to choose
the lit setting with Fn+Space. It queries again and proceeds only when both
CA and CC identify profile 2. It does not guess how to restore profile 0.

### Visible success and restoration accounting

The user selected profile 2 with Fn+Space and ran the static test. Stored test
payload matched, and the user reported **red**. This is the first confirmed
visible success of the installer-derived OneZone static sequence. Reports are
retained in `/tmp/legion-static-onezone-sjxv_8zy`. The sequence is AA1, CB
static animation 11 / colour red / LED ID 1, C8 profile 2, CE brightness 9,
D0 DC/profile 2 (150 ms between writes). The test does not isolate which
individual activation commands are necessary. ACPI ownership was not changed.

The initial restoration check returned false. Comparing all 960 bytes showed
exactly **one difference**, offset 2: original C0 → restored BC. This is the
known payload-length clamp from 960 to 956. Every profile data byte was
restored. The diagnostic now checks against the expected normalized header
and also records the strict raw equality separately. Physical blue restoration
was not independently reported.

onezone-ctl.py provides the working static
sequence for an arbitrary uniform RGB colour and brightness 1–9. It checks
model and HID identity, backs up queries before colour writes, changes stored
profile 2, and checks stored readback. Offline syntax and packet regression
checks passed. Only red has been visually verified with this exact sequence;
the production Rust/GUI encoder remains the old 24-zone implementation.

## Rust GUI and CLI integration

The user subsequently confirmed that the reusable command sets both colours
and brightness. The Rust encoder now matches its complete five-report sequence,
profile 2, single LED ID 1, static properties, and 150 ms spacing. The model
and GUI expose one zone; old version-1 24-zone presets load their first colour
and save as one zone. Normal Off remains CE zero without a profile write.
Brightness selection enables lighting. Opening the app still performs no writes.

The protocol regression test compares all five reports with the verified red
sequence including zero padding; additional tests cover arbitrary colour, all
brightness steps, Off, and legacy preset compatibility. The full test suite
(13 tests) and GUI compile check passed. Rust/GUI visual confirmation remains
separate from the user's successful Python hardware test.

## Native OneZone effects and Fn+Space profiles

The user confirmed Rust GUI static colour and brightness, then requested the
original changing-colour/blinking options and hotkey cycle. I returned to the
Windows DLL rather than adding host-generated animation frames.

`Device_OneZone` constructor `0x1800a3960` registers exactly JSON animation IDs
0, 21, 11. The factory `0x1800b9510`, case 21, constructs
`AnimationColorPulse24Zone`; despite its class name, it is explicitly selected
by OneZone's own supported list and shipped defaults. Its virtual slot 0x30
is `0x1800be410`, which returns **4**. The serializer `0x1800a6220` calls that
slot, so the correct wire effect is 4, not JSON ID 21. AnimationNone returns
zero through that slot, which the serializer translates to FF. Static is 11.
The capability map initializer `0x18008a870` gives animation 21 the speed and
colorType properties; unsupported clockwise/direction default to 0/4.

The shipped `default_profile_onezone.json` assigns profile 1 animation 21,
speed 2, colourType 1 (firmware colours), profile 2 static with colourType 2,
and profile 3 animation 0 (off). The encoder supports the fixed-colour variant
of pulse as Breathe and the firmware-colour variant as Colour cycle. Both use
wire effect 4 / speed 2; fixed-colour type 2 includes an RGB triple, while
firmware-colour type 1 includes no RGB triples. Every group addresses only LED 1.
Selected decompiled functions and the shipped defaults are retained in
[onezone-effects.txt](installer-evidence/onezone-effects.txt).

The GUI adds Static/Breathe/Colour cycle and Set Fn+Space cycle. The cycle
action saves 1 = firmware-colour pulse, 2 = the user's static colour, 3 = off,
then selects profile 2. Effect selection updates only its corresponding slot.
All writes retain AA1, firmware control D0 DC, and 150 ms spacing; no recurring
USB animation writes or ACPI ownership changes are used. Explicit cycle setup
commands are not discarded by edit debouncing. Old presets default to Static;
new presets preserve the effect. Startup still sends nothing.

All 16 Rust tests passed, including golden animated/off profiles, three-slot
setup, unchanged static packets, and effect preset compatibility. GUI compile
checks passed. These new animated modes and the actual Fn+Space cycle have
**not yet been visually verified**; source-derived configuration is not a
claim of runtime success.

### Breathing waveform and omitted settings review

The user asked whether breathing should fade smoothly and what Windows options
were omitted. Decompilation of `AnimationColorPulse::` tick `0x1800be240`
confirms a bidirectional intensity ramp: its direction reverses at peak and
reverses again below -9. RGB evaluation `0x1800be050` clamps negative phase to
zero and multiplies each channel by phase/peak. This establishes a fade up/down
with a brief dark interval in Lenovo's host renderer. It does not prove the
USB firmware renderer reproduces that waveform. Both functions are preserved
in the effect evidence. Abrupt blinking must not be presented as verified smooth
breathing or fixed merely by changing a label.

The app had omitted speed control (fixed at 2), custom RGB entry, multiple
colour entries (the OneZone serializer limits encoded entries to four), and
editing each hotkey slot independently. The shared factory also has rainbow,
colour change, wave, smooth, rain, ripple, audio, and typing implementations,
but these are not in the OneZone constructor's supported animation list.
Library availability is not evidence they work on this keyboard.

The GUI now exposes speed values 1/2/3 as Slow/Normal/Fast for animated effects,
retaining the shipped default 2. These are the conventional three speeds
suggested by the renderer's step = speed and delay = abs(3-speed)*10 ms; no
Windows UI range was separately captured. Profiles and presets preserve the
value, old presets default to 2, and static encoding still uses speed 0.
The 17 Rust tests passed. Speed-dependent firmware appearance remains unverified.

## Complete OneZone controls and compact application

At the user's request, the GUI now includes custom hex/RGB entry, 2–4-entry
fixed-colour pulse palettes, independent drafts for firmware slots 1–3, explicit
Apply/Save-all actions, and the shipped OneZone defaults. Profile tabs and loading
presets do not write. Model validation rejects invalid slots, speed and palette
lengths before sending. All three GUI slots can encode the supported off effect;
CLI Off remains brightness zero without replacing its saved profile. Brightness
is global to the keyboard, with the selected draft determining the sent value.

The implementation follows SeeMyMac's `remote-desktop-client-design.zip` README
§9 tokens and its current Linux chrome/control conventions in `src/view.rs`:
Inter/13px, 32px controls, 6px control radii, dark surfaces, blue focus outlines,
client-side header, drag/maximize, resize handles, and controls that stop the
header's drag event. This app uses 640×640 rather than the remote desktop app's
1280×800. Exit is visible in the header; Ctrl+Q also exits.

All drafts save atomically to the existing preset path. Old single-profile and
24-zone presets migrate into profile 2; new saves contain all three profiles,
palettes and selection. The local default arrangement is not sent at startup.
The Lenovo defaults action writes source-derived profile 1 automatic pulse /
profile 2 purple static / profile 3 off and selects profile 2.

The 22 model/protocol tests passed, including all palette lengths, independent
slot activation, off-slot encoding and preset compatibility. Three GPUI tests
also passed: controls fit at 640×640 and 560×480, Exit completes without starting
a header drag, text selection/editing/Enter works, and palette drafts remain
independent. Interface tests replace the USB worker with a disconnected channel;
no test sends hardware reports. New palette and slot playback still require
visual confirmation on the real keyboard.

## Custom palette playback failure

The user reports all new controls work except Custom palette, which flickers
only its first colour. With the custom effect active on profile 3, CA returned
3 and CC returned effect 4, speed 2, fixed-colour type 2, colour count **2**,
and RGB entries **88,5,255** and **0,80,255**, followed by one LED ID 1.
The exact readback is `/tmp/legion-custom-palette-active.bin`. Both colours
were stored, so the observed failure is not a dropped entry in the encoder.

A bounded live A1 red test on this now-valid profile (AA1 / D0 04/3 / A1 ID 1
red / empty A8) produced **dark**, per the user. D0 DC/3 and AA1 were restored;
CC was byte-for-byte unchanged. Script:
valid-profile live test.
No CB profile write or ownership change was made in that test.

Further decompilation confirms OneZone init `0x1800a3b80` opens PID C195
for chip type 1 or C693 for type 2, rather than this laptop's C615. This
compatibility gap means the shared encoder's ability to serialize four colours
is not proof of C615 palette playback. Earlier support claims were too strong.
The privileged ownership diagnostic now validates the active OneZone profile
and optionally tests both vendor A1 and standard LampArray red paths. This
retest differs from prior ownership tests, which predated correction of the
malformed stored profile. No application fallback or further guessed profile
write has been introduced.

The first updated ownership diagnostic stopped at its guard after setting
ownership 0 → 1, then unloaded the probe. Result:
`/tmp/legion-owner-result-31xc0izx.json`. No live packets were sent, so this
is not another negative live-colour result. A subsequent read returned selected
profile 0 and stored profile 3 with three colours; its returned key ID was 0.
The old script neither recorded these guard inputs nor checked them before
changing ownership.

The diagnostic now snapshots selection/data/brightness before changing ownership,
prompts for Apply profile if selection is 0, records selection after the change,
and explicitly selects the known original slot before entering app control.
The stored key bytes are diagnostic metadata rather than a prerequisite for
A1/LampArray testing (neither path parses or rewrites CB). Cleanup unloads the
ownership probe first, then restores selection/brightness and checks CC.
Syntax checks passed; this revised diagnostic has not yet been run as root.

## Custom palettes: working LampArray path

The corrected root diagnostic established that changing WMI ownership 0 → 1
changes selected profile 3 → 0. Vendor A1 red still produced **dark**, while
the standard LampArray path produced **red**. Probe unload restored ownership,
selected profile 3 was restored, and CC remained unchanged. User result:
`/tmp/legion-owner-result-tqk_a723.json`.

Crucially, a further non-root test worked without changing WMI ownership:
AA wire 0, standard control `06 00`, range update
`05 01 00 00 00 00 00 00 FF 01` displayed **solid blue**, per the user.
The script restored `06 01`, AA1, original selection/brightness and D0 DC; CC
was unchanged. This proves ACPI ownership changes are unnecessary for this
LampArray path; previous standard-interface tests omitted the AA0 channel
selection or mixed it with vendor app control. The diagnostic is retained as
test-lamparray-direct.py.

The custom palette now uses this verified standard interface. Its saved CB
profile is a static first-colour fallback, not the faulty multi-colour native
pulse. The worker plays each palette entry with a smooth intensity envelope,
scaling RGB for brightness and respecting the device's minimum update interval
(at least 50 ms). Playback uses only ten-byte standard range updates; it does
not repeatedly rewrite CB profiles. A native-profile change detected through
CA stops the player. All writes remain serialized with profile updates.

Exit, Ctrl+Q and normal window-close request worker shutdown and wait for
acknowledgement after LampArray autonomy, AA1, selection, brightness and D0 DC
have been restored. A live-session drop also attempts all restoration steps
after errors. Custom palettes require the app running; native effects and the
first-colour fallback continue after exit. Presets retain the full palette.

A temporary Rust program using the production player was run against the
keyboard. The user confirmed **red → green → blue all fade**. On stop, the
player restored firmware control and its saved CC report was unchanged. This
provides runtime confirmation of the Rust path as well as the earlier raw
standard-report tests. Unit tests additionally check every colour, wraparound,
dark colour-transition boundaries, speed timing, and brightness scaling.

Final verification: **24 model/protocol/player tests and 4 GPUI tests passed**.
The added interface test verifies Exit sends Shutdown, remains waiting before
the worker acknowledgement, and only completes after that acknowledgement.
The GUI and CLI binaries were rebuilt successfully. Structured confirmation is
retained in [lamparray-confirmation.json](installer-evidence/lamparray-confirmation.json).
