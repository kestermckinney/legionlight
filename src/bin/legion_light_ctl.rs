//! `legion_light_ctl` — probe and drive the Lenovo Legion keyboard backlight.
//!
//! Phase 0 of the plan in `DESIGN.md`: prove the wire before any UI exists.
//!
//! Writing is opt-in. `apply` and `off` print the exact reports they would send
//! and stop; `--write` is the only way anything reaches the controller.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use legion_light::lighting::{hid, protocol};
use legion_light::model::{LightingState, Rgb, BRIGHTNESS_MAX, BRIGHTNESS_MIN, ZONE_COUNT};
use legion_light::sysfs;

const REPORT_SPACING: Duration = protocol::REPORT_SPACING;

const USAGE: &str = "\
legion_light_ctl — probe and drive the Lenovo Legion (048d:c615) keyboard backlight

USAGE
  legion_light_ctl probe  [--json] [--device PATH]
  legion_light_ctl apply  [--all R,G,B] [--zone N=R,G,B]... [--brightness 1..9]
                          [--write] [--device PATH]
  legion_light_ctl off    [--write] [--device PATH]
  legion_light_ctl clear  [--write] [--device PATH]
  legion_light_ctl raw    HEXBYTES [--write] [--device PATH]
  legion_light_ctl doctor [--json]

Writes are off by default: every command prints the exact reports it would send.
Pass --write to put them on the wire.

  probe    read-only: identity, descriptor, feature reports, firmware replies
  apply    one keyboard colour: AA1 -> CB -> C8 -> CE -> D0 DC
  off      CE 00 — dims the current frame, keeps the stored profile
  clear    C8 -> CB (blank table) — destroys the profile Fn+Space replays
  raw      one feature report 0x07 from hex, zero-padded to 960 bytes
  doctor   text report for a bug report; performs no writes

This keyboard has one RGB zone (zone 1). Brightness is the firmware's raw step, 1..=9.

EXAMPLES
  legion_light_ctl probe
  legion_light_ctl apply --all 20,20,20
  legion_light_ctl apply --all 20,20,20 --write
  legion_light_ctl apply --zone 1=255,255,255 --brightness 9 --write
  legion_light_ctl off --write
  legion_light_ctl raw 07ce010000 --write
";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse(&argv) {
        Ok(None) => {
            print!("{USAGE}");
            0
        }
        Ok(Some(options)) => dispatch(options),
        Err(message) => {
            eprintln!("legion_light_ctl: {message}");
            eprintln!();
            eprint!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

struct Options {
    op: Op,
    device: Option<PathBuf>,
    write: bool,
    json: bool,
}

enum Op {
    Probe,
    Apply { all: Option<Rgb>, zones: Vec<(u8, Rgb)>, brightness: Option<u8> },
    Off,
    /// Destroy the stored zone table. Documented as a footgun, not a normal off.
    Clear,
    /// One hand-built feature report, for experiments the typed commands do not
    /// cover. Hazardous by definition; still behind `--write` and the device gate.
    Raw { bytes: Vec<u8> },
    Doctor,
}

fn parse(argv: &[String]) -> Result<Option<Options>, String> {
    let Some(command) = argv.first() else {
        return Ok(None);
    };

    let mut op = match command.as_str() {
        "probe" => Op::Probe,
        "apply" => Op::Apply { all: None, zones: Vec::new(), brightness: None },
        "off" => Op::Off,
        "clear" => Op::Clear,
        "raw" => Op::Raw { bytes: Vec::new() },
        "doctor" => Op::Doctor,
        "help" | "-h" | "--help" => return Ok(None),
        other => return Err(format!("unknown command `{other}`")),
    };

    let mut device = None;
    let mut write = false;
    let mut json = false;
    let mut rest = argv[1..].iter();

    while let Some(argument) = rest.next() {
        // `raw` is the one command with a positional argument.
        if let Op::Raw { bytes } = &mut op {
            if !argument.starts_with('-') {
                bytes.extend(parse_hex(argument)?);
                continue;
            }
        }

        match argument.as_str() {
            "--write" => write = true,
            "--dry-run" => write = false,
            "--json" => json = true,
            "--device" => {
                let value = rest.next().ok_or("--device needs a path")?;
                device = Some(PathBuf::from(value));
            }
            "--all" => {
                let value = rest.next().ok_or("--all needs R,G,B")?;
                let colour = Rgb::parse(value)?;
                match &mut op {
                    Op::Apply { all, .. } => *all = Some(colour),
                    _ => return Err("--all only applies to `apply`".into()),
                }
            }
            "--zone" => {
                let value = rest.next().ok_or("--zone needs N=R,G,B")?;
                let (zone, colour) = value
                    .split_once('=')
                    .ok_or_else(|| format!("--zone needs N=R,G,B, got `{value}`"))?;
                let zone: u8 = zone
                    .trim()
                    .parse()
                    .map_err(|_| format!("zone is not a number: `{zone}`"))?;
                if zone == 0 || zone as usize > ZONE_COUNT {
                    return Err(format!("zone {zone} is outside 1..={ZONE_COUNT}"));
                }
                let colour = Rgb::parse(colour)?;
                match &mut op {
                    Op::Apply { zones, .. } => zones.push((zone, colour)),
                    _ => return Err("--zone only applies to `apply`".into()),
                }
            }
            "--brightness" => {
                let value = rest.next().ok_or("--brightness needs 1..9")?;
                let brightness: u8 = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("brightness is not a number: `{value}`"))?;
                if !(BRIGHTNESS_MIN..=BRIGHTNESS_MAX).contains(&brightness) {
                    return Err(format!(
                        "brightness {brightness} is outside {BRIGHTNESS_MIN}..={BRIGHTNESS_MAX}"
                    ));
                }
                match &mut op {
                    Op::Apply { brightness: slot, .. } => *slot = Some(brightness),
                    _ => return Err("--brightness only applies to `apply`".into()),
                }
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    Ok(Some(Options { op, device, write, json }))
}

fn dispatch(options: Options) -> i32 {
    let Options { op, device, write, json } = options;
    match op {
        Op::Probe => probe(device.as_deref(), json),
        Op::Apply { all, zones, brightness } => apply(device.as_deref(), write, all, zones, brightness),
        Op::Off => off(device.as_deref(), write),
        Op::Clear => clear(device.as_deref(), write),
        Op::Raw { bytes } => raw(device.as_deref(), write, bytes),
        Op::Doctor => doctor(json),
    }
}

/// Parse an even-length ASCII hex string, e.g. `07ce010000`.
fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.is_ascii() {
        return Err(format!("`{text}` is not hex"));
    }
    if text.len() % 2 != 0 {
        return Err(format!("hex needs an even number of digits, got `{text}`"));
    }
    (0..text.len() / 2)
        .map(|index| {
            let pair = &text[index * 2..index * 2 + 2];
            u8::from_str_radix(pair, 16).map_err(|_| format!("`{pair}` is not hex in `{text}`"))
        })
        .collect()
}

// ---------------------------------------------------------------- probe ------

enum Reply {
    Bytes(Vec<u8>),
    Failed(String),
}

struct Finding {
    path: String,
    name: Option<String>,
    id: String,
    interface: Option<String>,
    descriptor_len: usize,
    signature_at: Option<usize>,
    feature_reports: Vec<(u8, usize)>,
    lighting: bool,
    replies: Vec<(u8, Reply)>,
}

fn probe(device: Option<&Path>, json: bool) -> i32 {
    let nodes = match device {
        Some(path) => vec![path.to_path_buf()],
        None => hid::nodes(),
    };
    if nodes.is_empty() {
        eprintln!("no hidraw interfaces found under /sys/class/hidraw");
        return 1;
    }

    let mut findings = Vec::new();
    for node in &nodes {
        match hid::Device::open(node) {
            Ok(device) => findings.push(inspect(&device)),
            Err(error) => eprintln!("skipping: {error}"),
        }
    }
    if findings.is_empty() {
        return 1;
    }

    if json {
        let value = serde_json::json!({
            "devices": findings.iter().map(finding_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
        return 0;
    }

    for finding in &findings {
        print_finding(finding);
    }

    if !findings.iter().any(|finding| finding.lighting) {
        eprintln!(
            "\nno interface passed the lighting gate (048d:c615, report 0x07, 959 bytes)"
        );
        return 1;
    }
    0
}

fn inspect(device: &hid::Device) -> Finding {
    let mut replies = Vec::new();
    if device.is_lighting() {
        for (report, length) in [(protocol::REPORT_ID, 959usize), (0x5A, 16), (0xCC, 32)] {
            let reply = match device.get_feature(report, length) {
                Ok(bytes) => Reply::Bytes(bytes),
                Err(error) => Reply::Failed(error.to_string()),
            };
            replies.push((report, reply));
        }
    }

    Finding {
        path: device.path.display().to_string(),
        name: device.hid_name.clone(),
        id: format!("{:04x}:{:04x}:{:04x}", device.bus, device.vendor, device.product),
        interface: device.interface.clone(),
        descriptor_len: device.descriptor.len(),
        signature_at: device.signature_at,
        feature_reports: device.feature_reports.iter().map(|(id, size)| (*id, *size)).collect(),
        lighting: device.is_lighting(),
        replies,
    }
}

fn print_finding(finding: &Finding) {
    let name = finding.name.as_deref().unwrap_or("(unnamed HID interface)");
    println!("{}  {name}", finding.path);
    println!("  id  {}   interface {}", finding.id, finding.interface.as_deref().unwrap_or("?"));
    match finding.signature_at {
        Some(offset) => println!(
            "  descriptor {} bytes; ITE lighting signature at offset {offset}",
            finding.descriptor_len
        ),
        None => println!("  descriptor {} bytes; no ITE lighting signature", finding.descriptor_len),
    }
    let reports: Vec<String> = finding
        .feature_reports
        .iter()
        .map(|(id, size)| format!("0x{id:02x} {size} B"))
        .collect();
    println!("  feature reports  {}", if reports.is_empty() { "none".into() } else { reports.join("   ") });
    println!("  lighting interface: {}", if finding.lighting { "yes" } else { "no" });

    for (report, reply) in &finding.replies {
        match reply {
            Reply::Bytes(bytes) => println!("  GET_FEATURE 0x{report:02x}: {}", hex_head(bytes, 32)),
            Reply::Failed(message) => println!("  GET_FEATURE 0x{report:02x}: failed: {message}"),
        }
    }
    println!();
}

fn finding_json(finding: &Finding) -> serde_json::Value {
    let replies: Vec<serde_json::Value> = finding
        .replies
        .iter()
        .map(|(report, reply)| match reply {
            Reply::Bytes(bytes) => serde_json::json!({
                "report": format!("{report:#04x}"),
                "len": bytes.len(),
                "bytes": bytes.iter().take(32).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
            }),
            Reply::Failed(message) => serde_json::json!({
                "report": format!("{report:#04x}"),
                "error": message,
            }),
        })
        .collect();

    serde_json::json!({
        "path": finding.path,
        "name": finding.name,
        "id": finding.id,
        "interface": finding.interface,
        "descriptor_len": finding.descriptor_len,
        "signature_at": finding.signature_at,
        "feature_reports": finding.feature_reports
            .iter()
            .map(|(id, size)| serde_json::json!({ "report": format!("{id:#04x}"), "bytes": size }))
            .collect::<Vec<_>>(),
        "lighting": finding.lighting,
        "replies": replies,
    })
}

// ---------------------------------------------------------------- apply ------

fn apply(
    device: Option<&Path>,
    write: bool,
    all: Option<Rgb>,
    zones: Vec<(u8, Rgb)>,
    brightness: Option<u8>,
) -> i32 {
    let has_zone_work = all.is_some() || !zones.is_empty();
    if !has_zone_work {
        eprintln!("nothing to apply: pass --all R,G,B or --zone N=R,G,B");
        return 2;
    }

    let mut state = LightingState::default();
    if let Some(brightness) = brightness {
        state.brightness = brightness;
    }
    if let Some(colour) = all {
        state.zones = [colour; ZONE_COUNT];
    }
    for (zone, colour) in zones {
        if let Err(message) = state.set_zone(zone, colour) {
            eprintln!("{message}");
            return 2;
        }
    }

    send_state(device, write, &state)
}

fn off(device: Option<&Path>, write: bool) -> i32 {
    let state = LightingState { enabled: false, ..LightingState::default() };
    send_state(device, write, &state)
}

/// The destructive off, kept explicit because it removes the only thing Fn+Space
/// can turn back on.
fn clear(device: Option<&Path>, write: bool) -> i32 {
    let reports = protocol::clear_reports();

    println!("clear: blank zone table (this is the destructive one)");
    println!("after this Fn+Space has no stored frame to turn on\n");
    for (index, report) in reports.iter().enumerate() {
        println!("[{}] {}", index + 1, report_name(report));
        print!("{}", preview(report));
    }

    if !write {
        println!("\ndry run: nothing written. Pass --write to send this sequence.");
        return 0;
    }

    write_reports(device, &reports)
}

fn send_state(device: Option<&Path>, write: bool, state: &LightingState) -> i32 {
    let reports = match protocol::reports_for(state) {
        Ok(reports) => reports,
        Err(error) => {
            eprintln!("cannot encode: {error}");
            return 2;
        }
    };

    println!(
        "{} zones, {} distinct colour(s), brightness {}{}",
        ZONE_COUNT,
        state.distinct_colours(),
        state.brightness,
        if state.enabled { "" } else { ", off" }
    );
    println!("{} reports of {} bytes\n", reports.len(), protocol::REPORT_LEN);

    for (index, report) in reports.iter().enumerate() {
        println!("[{}] {}", index + 1, report_name(report));
        print!("{}", preview(report));
    }

    if !write {
        println!("\ndry run: nothing written. Pass --write to send this sequence.");
        return 0;
    }

    write_reports(device, &reports)
}

// ------------------------------------------------------------------ raw ------

fn raw(device: Option<&Path>, write: bool, bytes: Vec<u8>) -> i32 {
    if bytes.is_empty() {
        eprintln!("raw needs hex bytes, e.g. `raw 07ce010000`");
        return 2;
    }
    if bytes.len() > protocol::REPORT_LEN {
        eprintln!(
            "raw report is {} bytes; the feature buffer is {} bytes",
            bytes.len(),
            protocol::REPORT_LEN
        );
        return 2;
    }
    if bytes[0] != protocol::REPORT_ID {
        eprintln!(
            "raw only writes report {:#04x}; {:#04x} is not documented as writable on 048d:c615",
            protocol::REPORT_ID,
            bytes[0]
        );
        return 2;
    }

    let mut report = [0u8; protocol::REPORT_LEN];
    report[..bytes.len()].copy_from_slice(&bytes);

    println!("raw report: {} of {} bytes given", bytes.len(), protocol::REPORT_LEN);
    print!("{}", preview(&report));

    if !write {
        println!("\ndry run: nothing written. Pass --write to send this report.");
        return 0;
    }

    write_reports(device, &[report])
}

// ---------------------------------------------------------------- writer -----

/// The only place that opens a device for writing. Gates, spaces and reports.
fn write_reports(device: Option<&Path>, reports: &[[u8; protocol::REPORT_LEN]]) -> i32 {
    let path = match device {
        Some(path) => path.to_path_buf(),
        None => match hid::lighting_node() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("{error}");
                return 1;
            }
        },
    };
    let controller = match hid::Device::open_for_write(&path) {
        Ok(controller) => controller,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Err(error) = controller.ensure_lighting() {
        eprintln!("refusing to write: {error}");
        return 1;
    }

    println!("\nwriting to {} …", controller.path.display());
    let started = Instant::now();
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            std::thread::sleep(REPORT_SPACING);
        }
        let step = Instant::now();
        match controller.send(report) {
            Ok(()) => println!("  [{}] {} sent in {:?}", index + 1, report_name(report), step.elapsed()),
            Err(error) => {
                eprintln!("  [{}] {} failed: {error}", index + 1, report_name(report));
                return 1;
            }
        }
    }
    println!("done in {:?}", started.elapsed());
    0
}

fn report_name(report: &[u8]) -> String {
    match report.get(1).copied() {
        Some(protocol::CMD_SELECT_PROFILE) => "C8 select profile".into(),
        Some(protocol::CMD_SAVE_ZONES) => "CB zone table".into(),
        Some(protocol::CMD_BRIGHTNESS) => "CE brightness".into(),
        Some(other) => format!("command {other:#04x}"),
        None => "empty report".into(),
    }
}

/// One hex line per 16 bytes up to the last non-zero byte, capped so a 960-byte
/// report never floods a terminal.
fn preview(report: &[u8]) -> String {
    let used = report.iter().rposition(|byte| *byte != 0).map_or(0, |index| index + 1);
    let shown = used.min(96);
    let mut out = format!("  {used} of {} bytes used\n", report.len());

    for (row, chunk) in report[..shown].chunks(16).enumerate() {
        out.push_str(&format!("  {:04x}  ", row * 16));
        for (column, byte) in chunk.iter().enumerate() {
            out.push_str(&format!("{byte:02x} "));
            if column == 7 {
                out.push(' ');
            }
        }
        out.push('\n');
    }
    if used > shown {
        out.push_str("  …\n");
    }
    out
}

fn hex_head(bytes: &[u8], limit: usize) -> String {
    let shown: Vec<String> = bytes.iter().take(limit).map(|byte| format!("{byte:02x}")).collect();
    let mut text = shown.join(" ");
    if bytes.len() > limit {
        text.push_str(" …");
    }
    if text.is_empty() {
        text.push_str("(empty)");
    }
    text
}

// --------------------------------------------------------------- doctor ------

fn doctor(json: bool) -> i32 {
    let machine = sysfs::machine();
    let telemetry = sysfs::read();

    let mut interfaces = Vec::new();
    let mut lighting: Option<Finding> = None;
    for node in hid::nodes() {
        let Ok(device) = hid::Device::open(&node) else {
            continue;
        };
        let finding = inspect(&device);
        if finding.lighting {
            lighting = Some(Finding { replies: Vec::new(), ..finding });
        } else {
            interfaces.push(finding);
        }
    }

    if json {
        let value = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "unix_time": unix_time(),
            "machine": {
                "product": machine.product,
                "board": machine.board,
                "kernel": machine.kernel,
            },
            "lighting": lighting.as_ref().map(|f| serde_json::json!({
                "path": f.path,
                "id": f.id,
                "interface": f.interface,
                "descriptor_len": f.descriptor_len,
                "signature_at": f.signature_at,
                "feature_reports": f.feature_reports
                    .iter()
                    .map(|(id, size)| serde_json::json!({ "report": format!("{id:#04x}"), "bytes": size }))
                    .collect::<Vec<_>>(),
            })),
            "other_hidraw_interfaces": interfaces.len(),
            "sysfs": {
                "platform_profile": telemetry.platform_profile,
                "platform_profile_choices": telemetry.platform_profile_choices,
                "fn_lock": telemetry.fn_lock,
                "fan_mode": telemetry.fan_mode,
                "conservation_mode": telemetry.conservation_mode,
            },
            "sensors": telemetry.sensors.iter().map(|sensor| serde_json::json!({
                "chip": sensor.chip,
                "label": sensor.label,
                "kind": match sensor.kind {
                    sysfs::SensorKind::Temperature => "temperature",
                    sysfs::SensorKind::Fan => "fan",
                },
                "value": sensor.value,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
        return 0;
    }

    println!("LegionForGnome diagnostic report (no writes performed)");
    println!("legion_light_ctl {}", env!("CARGO_PKG_VERSION"));
    println!("time         unix {}", unix_time());
    println!(
        "kernel       {}",
        machine.kernel.as_deref().unwrap_or("unknown")
    );
    println!(
        "dmi          {} / {}",
        machine.product.as_deref().unwrap_or("unknown"),
        machine.board.as_deref().unwrap_or("unknown")
    );

    match &lighting {
        Some(finding) => {
            println!();
            println!("lighting     {}  {}", finding.path, finding.id);
            println!(
                "             interface {}  descriptor {} bytes  signature at {}",
                finding.interface.as_deref().unwrap_or("?"),
                finding.descriptor_len,
                finding.signature_at.map_or("none".to_string(), |offset| offset.to_string())
            );
            let reports: Vec<String> = finding
                .feature_reports
                .iter()
                .map(|(id, size)| format!("0x{id:02x}={size}B"))
                .collect();
            println!("             reports {}", reports.join(" "));
        }
        None => {
            println!();
            println!("lighting     NOT FOUND (expected 048d:c615 with report 0x07 = 959 B)");
            println!("             {} other hidraw interface(s) present", interfaces.len());
        }
    }

    println!();
    println!(
        "profile      {} (choices: {})",
        telemetry.platform_profile.as_deref().unwrap_or("unavailable"),
        telemetry.platform_profile_choices.as_deref().unwrap_or("unavailable")
    );
    println!(
        "conservation {}",
        telemetry.conservation_mode.as_deref().unwrap_or("unavailable")
    );
    println!("fn_lock      {}", telemetry.fn_lock.as_deref().unwrap_or("unavailable"));
    println!("fan_mode     {}", telemetry.fan_mode.as_deref().unwrap_or("unavailable"));

    println!();
    if telemetry.sensors.is_empty() {
        println!("sensors      none found under {}", sysfs::HWMON);
    } else {
        println!("sensors");
        for sensor in &telemetry.sensors {
            let value = match sensor.kind {
                sysfs::SensorKind::Temperature => format!("{:.1} C", sensor.value),
                sysfs::SensorKind::Fan => format!("{:.0} rpm", sensor.value),
            };
            println!("             {:<12} {:<8} {}", sensor.chip, sensor.label, value);
        }
    }

    println!();
    if let Some(sensor) = telemetry
        .sensors
        .iter()
        .find(|sensor| sensor.kind == sysfs::SensorKind::Fan && sensor.value > 20_000.0)
    {
        println!(
            "note         {} {} reads {:.0}, far above any real fan speed;",
            sensor.chip, sensor.label, sensor.value
        );
        println!(
            "             this BIOS reports acpi_fan in 0.01 rpm units (≈{:.0} rpm)",
            sensor.value / 100.0
        );
    }
    if Path::new("/etc/udev/rules.d/60-legionaura.rules").exists() {
        println!("note         /etc/udev/rules.d/60-legionaura.rules grants MODE=\"0666\"");
        println!("             on 048d:c615; replace it with a uaccess rule when packaging");
    }
    if !sysfs::has_platform_profile() {
        println!("note         no /sys/firmware/acpi/platform_profile; performance page unavailable");
    }

    0
}

fn unix_time() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
}
