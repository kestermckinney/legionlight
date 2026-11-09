//! LegionForGnome: keyboard backlight and performance control for Lenovo Legion
//! laptops.
//!
//! The crate is split so that the parts that can be reasoned about without a
//! laptop stay free of `gpui` and of `libc`:
//!
//! * [`model`] — plain data types for a lighting configuration.
//! * [`lighting::protocol`] — the ITE 8258 feature-report encoder. Pure.
//! * [`lighting::descriptor`] — the HID report-descriptor reader used as part of
//!   the device safety gate. Pure.
//! * [`lighting::hid`] — discovery, identity checks and the ioctl transport.
//! * [`sysfs`] — read-only firmware and sensor state.

pub mod lighting;
pub mod model;
pub mod sysfs;
