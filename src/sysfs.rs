//! Read-only firmware, machine and sensor state.
//!
//! This module never writes. The four switches it reads are root-writable sysfs
//! attributes, so the GUI's later daemon will be the only writer; keeping reads
//! here means `doctor` and the UI agree on how a value is discovered.

use std::fs;
use std::path::{Path, PathBuf};

pub const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";
pub const PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";
/// Lenovo's platform device for conservation mode, Fn-lock and fan mode.
pub const VPC: &str = "/sys/bus/platform/devices/VPC2004:00";
pub const HWMON: &str = "/sys/class/hwmon";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    Temperature,
    Fan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sensor {
    pub chip: String,
    pub label: String,
    pub kind: SensorKind,
    /// Degrees Celsius for temperatures, RPM for fans.
    pub value: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Telemetry {
    pub platform_profile: Option<String>,
    pub platform_profile_choices: Option<String>,
    pub fn_lock: Option<String>,
    pub fan_mode: Option<String>,
    pub conservation_mode: Option<String>,
    pub sensors: Vec<Sensor>,
}

impl Telemetry {
    pub fn sensor(&self, chip: &str, kind: SensorKind) -> Option<&Sensor> {
        self.sensors.iter().find(|sensor| sensor.chip == chip && sensor.kind == kind)
    }
}

/// Machine identity, for the diagnostic report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Machine {
    pub product: Option<String>,
    pub board: Option<String>,
    pub kernel: Option<String>,
}

pub fn machine() -> Machine {
    Machine {
        product: text("/sys/class/dmi/id/product_name"),
        board: text("/sys/class/dmi/id/board_name"),
        kernel: text("/proc/sys/kernel/osrelease"),
    }
}

pub fn read() -> Telemetry {
    Telemetry {
        platform_profile: text(PLATFORM_PROFILE),
        platform_profile_choices: text(PLATFORM_PROFILE_CHOICES),
        fn_lock: attribute("fn_lock"),
        fan_mode: attribute("fan_mode"),
        conservation_mode: attribute("conservation_mode"),
        sensors: sensors(),
    }
}

/// Value of one `VPC2004:00` switch, if this machine publishes it.
pub fn attribute(name: &str) -> Option<String> {
    text(Path::new(VPC).join(name))
}

/// Whether the platform profile attribute is readable, i.e. whether the
/// `lenovo-wmi-gamezone` driver bound.
pub fn has_platform_profile() -> bool {
    Path::new(PLATFORM_PROFILE).exists()
}

fn text(path: impl AsRef<Path>) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn sensors() -> Vec<Sensor> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(HWMON) else {
        return out;
    };

    let mut chips: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    chips.sort();

    for chip_dir in chips {
        let chip = text(chip_dir.join("name")).unwrap_or_else(|| "unknown".into());
        let Ok(files) = fs::read_dir(&chip_dir) else {
            continue;
        };
        let mut names: Vec<String> = files
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();

        for name in names {
            let (kind, rest) = if let Some(rest) = name.strip_prefix("temp") {
                (SensorKind::Temperature, rest)
            } else if let Some(rest) = name.strip_prefix("fan") {
                (SensorKind::Fan, rest)
            } else {
                continue;
            };
            // Only the readings, not the limit attributes.
            if !rest.ends_with("_input") {
                continue;
            }
            let number = rest.trim_end_matches("_input");
            let label = match kind {
                SensorKind::Temperature => format!("temp{number}"),
                SensorKind::Fan => format!("fan{number}"),
            };
            let Ok(raw) = fs::read_to_string(chip_dir.join(&name)) else {
                continue;
            };
            let Ok(raw) = raw.trim().parse::<f64>() else {
                continue;
            };
            let value = match kind {
                SensorKind::Temperature => raw / 1000.0,
                SensorKind::Fan => raw,
            };
            out.push(Sensor { chip: chip.clone(), label, kind, value });
        }
    }
    out
}
