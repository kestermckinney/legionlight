//! Discovery, identity checks and the hidraw transport.
//!
//! The only write path in the program. Everything it accepts is checked first,
//! because the alternative is sending a 960-byte "reconfigure yourself" packet to
//! whatever happens to be at `/dev/hidraw0`.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::lighting::descriptor;

/// USB bus type reported by `HIDIOCGRAWINFO`.
pub const HID_BUS_USB: u16 = 0x0003;
/// Lenovo's USB vendor ID.
pub const VENDOR_ID: u16 = 0x048D;
/// The ITE 8258 keyboard controller on the Legion 5 15AHP11.
pub const PRODUCT_ID: u16 = 0xC615;

/// The lighting command report.
pub const LIGHTING_REPORT: u8 = 0x07;
/// Its payload size in bytes, excluding the report ID.
pub const LIGHTING_PAYLOAD: usize = 959;

/// Largest report descriptor we will read.
pub const MAX_DESCRIPTOR: usize = 4096;

const SYSFS_HIDRAW: &str = "/sys/class/hidraw";
const DEV: &str = "/dev";

// _IOC(dir, 'H', nr, len). dir 3 = READ|WRITE.
const fn ioc(direction: u64, number: u64, length: usize) -> libc::c_ulong {
    (direction << 30 | (length as u64) << 16 | (b'H' as u64) << 8 | number) as libc::c_ulong
}

const fn iocgfeature(length: usize) -> libc::c_ulong {
    ioc(3, 0x07, length)
}

const fn iocsfeature(length: usize) -> libc::c_ulong {
    ioc(3, 0x06, length)
}

/// `HIDIOCGRAWINFO`: `{ __u32 bustype; __s16 vendor; __s16 product; }`.
const HIDIOCGRAWINFO: libc::c_ulong = ioc(2, 0x03, 8);
/// `HIDIOCGRDESCSIZE`: `__u32`.
const HIDIOCGRDESCSIZE: libc::c_ulong = ioc(2, 0x01, 4);
/// `HIDIOCGRDESC`: `__u32` length followed by that many descriptor bytes.
const HIDIOCGRDESC: libc::c_ulong = ioc(2, 0x02, 4 + MAX_DESCRIPTOR);

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no /dev/hidraw node matches 048d:c615")]
    NotFound,

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path}: not the keyboard controller (bus {bus:#06x} id {vendor:#06x}:{product:#06x})")]
    WrongIdentity { path: PathBuf, bus: u16, vendor: u16, product: u16 },

    #[error("{path}: HID descriptor has no ITE lighting collection")]
    WrongProtocol { path: PathBuf },

    #[error("{path}: feature report {report:#04x} is {got} bytes, expected {want}")]
    WrongReport { path: PathBuf, report: u8, want: usize, got: usize },
}

/// An open hidraw node with everything the safety gate needs already read.
pub struct Device {
    fd: OwnedFd,
    pub path: PathBuf,
    /// Canonical `/sys/class/hidraw/<name>/device`, when sysfs is available.
    pub sysfs: Option<PathBuf>,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    /// `bInterfaceNumber` of the USB interface, when sysfs exposes it.
    pub interface: Option<String>,
    pub hid_name: Option<String>,
    pub descriptor: Vec<u8>,
    pub feature_reports: BTreeMap<u8, usize>,
    pub signature_at: Option<usize>,
}

impl Device {
    /// Open read-only. Safe on any HID node: nothing is written and no gate is
    /// applied, so this is what `probe` uses to describe every interface.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DeviceError> {
        Self::open_with(path.as_ref(), false)
    }

    /// Open for writing. The gate is still [`Self::ensure_lighting`]; this only
    /// asks the kernel for write access.
    pub fn open_for_write(path: impl AsRef<Path>) -> Result<Self, DeviceError> {
        Self::open_with(path.as_ref(), true)
    }

    fn open_with(path: &Path, write: bool) -> Result<Self, DeviceError> {
        // O_NOFOLLOW: a symlinked /dev/hidraw0 must not turn into "write anywhere".
        let file = OpenOptions::new()
            .read(true)
            .write(write)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|source| DeviceError::Io { path: path.to_path_buf(), source })?;

        let fd = OwnedFd::from(file);
        let raw = fd.as_raw_fd();
        let io = |source: io::Error| DeviceError::Io { path: path.to_path_buf(), source };

        let (bus, vendor, product) = raw_info(raw).map_err(io)?;
        let descriptor = report_descriptor(raw).map_err(io)?;
        let feature_reports = descriptor::feature_reports(&descriptor);
        let signature_at = descriptor::find_signature(&descriptor);

        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let sysfs = sysfs_device(&name);
        let interface = sysfs.as_deref().and_then(interface_number);
        let hid_name = sysfs.as_deref().and_then(hid_name);

        Ok(Self {
            fd,
            path: path.to_path_buf(),
            sysfs,
            bus,
            vendor,
            product,
            interface,
            hid_name,
            descriptor,
            feature_reports,
            signature_at,
        })
    }

    /// True when every identity check passes: the USB IDs, the descriptor's
    /// lighting collection and the 959-byte report `0x07`.
    ///
    /// The descriptor check subsumes the interface-number check — interface 01
    /// (LampArray) cannot contain the vendor lighting collection — so a missing
    /// `bInterfaceNumber` in sysfs is not treated as a failure.
    pub fn is_lighting(&self) -> bool {
        self.bus == HID_BUS_USB
            && self.vendor == VENDOR_ID
            && self.product == PRODUCT_ID
            && self.signature_at.is_some()
            && self.feature_reports.get(&LIGHTING_REPORT).copied() == Some(LIGHTING_PAYLOAD)
    }

    /// Refuse to write unless [`Self::is_lighting`] holds.
    pub fn ensure_lighting(&self) -> Result<(), DeviceError> {
        if (self.bus, self.vendor, self.product) != (HID_BUS_USB, VENDOR_ID, PRODUCT_ID) {
            return Err(DeviceError::WrongIdentity {
                path: self.path.clone(),
                bus: self.bus,
                vendor: self.vendor,
                product: self.product,
            });
        }
        if self.signature_at.is_none() {
            return Err(DeviceError::WrongProtocol { path: self.path.clone() });
        }
        let got = self.feature_reports.get(&LIGHTING_REPORT).copied().unwrap_or(0);
        if got != LIGHTING_PAYLOAD {
            return Err(DeviceError::WrongReport {
                path: self.path.clone(),
                report: LIGHTING_REPORT,
                want: LIGHTING_PAYLOAD,
                got,
            });
        }
        Ok(())
    }

    /// The standard LampArray interface on the same confirmed keyboard model.
    pub fn is_lamparray(&self) -> bool {
        self.bus == HID_BUS_USB && self.vendor == VENDOR_ID && self.product == PRODUCT_ID
            && self.feature_reports.get(&1) == Some(&22)
            && self.feature_reports.get(&5) == Some(&9)
            && self.feature_reports.get(&6) == Some(&1)
    }

    /// `HIDIOCGFEATURE`. `length` includes the report ID byte.
    pub fn get_feature(&self, report: u8, length: usize) -> io::Result<Vec<u8>> {
        let mut buffer = vec![0u8; length];
        buffer[0] = report;
        let rc = unsafe {
            libc::ioctl(self.fd.as_raw_fd(), iocgfeature(length), buffer.as_mut_ptr())
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buffer)
    }

    /// `HIDIOCSFEATURE`. `report` is the whole buffer, report ID first.
    pub fn send(&self, report: &[u8]) -> io::Result<()> {
        let mut buffer = report.to_vec();
        let rc = unsafe {
            libc::ioctl(self.fd.as_raw_fd(), iocsfeature(buffer.len()), buffer.as_mut_ptr())
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Every `/dev/hidrawN` node the kernel currently exposes, in numeric order.
pub fn nodes() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(SYSFS_HIDRAW) else {
        return Vec::new();
    };
    let mut names: Vec<(u32, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let number = name.strip_prefix("hidraw")?.parse::<u32>().ok()?;
            Some((number, name))
        })
        .collect();
    names.sort();
    names.into_iter().map(|(_, name)| Path::new(DEV).join(name)).collect()
}

/// The first node that passes the full lighting gate.
pub fn lighting_node() -> Result<PathBuf, DeviceError> {
    for node in nodes() {
        if let Ok(device) = Device::open(&node) {
            if device.is_lighting() {
                return Ok(node);
            }
        }
    }
    Err(DeviceError::NotFound)
}

pub fn lamparray_node() -> Result<PathBuf, DeviceError> {
    for node in nodes() {
        if let Ok(device) = Device::open(&node) {
            if device.is_lamparray() { return Ok(node); }
        }
    }
    Err(DeviceError::NotFound)
}

fn raw_info(fd: i32) -> io::Result<(u16, u16, u16)> {
    let mut buffer = [0u8; 8];
    let rc = unsafe { libc::ioctl(fd, HIDIOCGRAWINFO, buffer.as_mut_ptr()) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    // bustype: u32; vendor: i16; product: i16 — read as unsigned, they are IDs.
    let bus = u16::from_ne_bytes([buffer[0], buffer[1]]);
    let vendor = u16::from_ne_bytes([buffer[4], buffer[5]]);
    let product = u16::from_ne_bytes([buffer[6], buffer[7]]);
    Ok((bus, vendor, product))
}

fn report_descriptor(fd: i32) -> io::Result<Vec<u8>> {
    let mut size = [0u8; 4];
    let rc = unsafe { libc::ioctl(fd, HIDIOCGRDESCSIZE, size.as_mut_ptr()) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    let length = u32::from_ne_bytes(size) as usize;
    if length == 0 || length > MAX_DESCRIPTOR {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("implausible HID report descriptor length {length}"),
        ));
    }

    let mut buffer = vec![0u8; 4 + MAX_DESCRIPTOR];
    buffer[..4].copy_from_slice(&(length as u32).to_ne_bytes());
    let rc = unsafe { libc::ioctl(fd, HIDIOCGRDESC, buffer.as_mut_ptr()) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer[4..4 + length].to_vec())
}

fn sysfs_device(name: &str) -> Option<PathBuf> {
    std::fs::canonicalize(Path::new(SYSFS_HIDRAW).join(name).join("device")).ok()
}

fn interface_number(device: &Path) -> Option<String> {
    // The HID device sits under the USB interface directory that owns it.
    for ancestor in device.ancestors().take(6) {
        if let Ok(value) = std::fs::read_to_string(ancestor.join("bInterfaceNumber")) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn hid_name(device: &Path) -> Option<String> {
    let uevent = std::fs::read_to_string(device.join("uevent")).ok()?;
    uevent
        .lines()
        .find_map(|line| line.strip_prefix("HID_NAME="))
        .map(|name| name.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_kernel_headers() {
        // HIDIOCGFEATURE(959) and HIDIOCSFEATURE(960) as computed by _IOC.
        assert_eq!(iocgfeature(959), 0xC000_0000u64 | (959 << 16) | (0x48 << 8) | 0x07);
        assert_eq!(iocsfeature(960), 0xC000_0000u64 | (960 << 16) | (0x48 << 8) | 0x06);
        assert_eq!(HIDIOCGRAWINFO, 0x80084803);
    }

    #[test]
    fn nodes_are_sorted_numerically() {
        // The real list depends on the machine; only ordering is the contract.
        let nodes = nodes();
        let numbers: Vec<u32> = nodes
            .iter()
            .filter_map(|path| path.file_name()?.to_string_lossy().strip_prefix("hidraw")?.parse().ok())
            .collect();
        let mut sorted = numbers.clone();
        sorted.sort();
        assert_eq!(numbers, sorted);
    }
}
