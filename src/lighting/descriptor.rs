//! A very small HID report-descriptor reader.
//!
//! It answers two questions and nothing else:
//!
//! 1. Which feature reports does this interface have, and how big is each payload?
//! 2. Does this descriptor contain the ITE lighting collection?
//!
//! Question 2 is the important half of the safety gate. Our controller's
//! descriptor reaches the lighting collection at offset 23, not offset 0, so the
//! check is a substring search — a prefix check would reject the very laptop this
//! program exists for.

use std::collections::BTreeMap;

/// The lighting collection header as it appears in the report descriptor:
/// Usage Page (Vendor 0xFF89), Usage 0x07, Collection (Application), Report ID 7.
pub const SIGNATURE: [u8; 9] = [0x06, 0x89, 0xFF, 0x09, 0x07, 0xA1, 0x01, 0x85, 0x07];

/// Offset of [`SIGNATURE`] inside `descriptor`, if present.
pub fn find_signature(descriptor: &[u8]) -> Option<usize> {
    descriptor.windows(SIGNATURE.len()).position(|window| window == SIGNATURE.as_slice())
}

/// Feature report id → payload size in bytes, excluding the one-byte report ID.
pub fn feature_reports(descriptor: &[u8]) -> BTreeMap<u8, usize> {
    #[derive(Clone, Copy, Default)]
    struct State {
        report_id: u8,
        report_size: usize,
        report_count: usize,
    }

    let mut feature: BTreeMap<u8, usize> = BTreeMap::new();
    let mut global = State::default();
    let mut stack: Vec<State> = Vec::new();
    let mut index = 0;

    while index < descriptor.len() {
        let prefix = descriptor[index];

        // Long item: 0xFE, then a size byte, then that many data bytes.
        if prefix == 0xFE {
            if index + 1 >= descriptor.len() {
                break;
            }
            index += 3 + descriptor[index + 1] as usize;
            continue;
        }

        let size = match prefix & 0x03 {
            3 => 4,
            other => other as usize,
        };
        let kind = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        let data_start = index + 1;
        let data_end = data_start + size;
        if data_end > descriptor.len() {
            break;
        }
        let value = descriptor[data_start..data_end]
            .iter()
            .rev()
            .fold(0usize, |accumulated, byte| (accumulated << 8) | *byte as usize);

        match kind {
            // Global items.
            1 => match tag {
                0x7 => global.report_size = value,  // Report Size
                0x8 => global.report_id = value as u8, // Report ID
                0x9 => global.report_count = value, // Report Count
                0xA => stack.push(global),          // Push
                0xB => global = stack.pop().unwrap_or_default(), // Pop
                _ => {}
            },
            // Main items.
            0 => {
                // Tag 0xB is Feature; Input is 0x8, Output is 0x9.
                if tag == 0xB {
                    *feature.entry(global.report_id).or_insert(0) += global.report_size * global.report_count;
                }
            }
            _ => {}
        }

        index = data_end;
    }

    feature.into_iter().map(|(id, bits)| (id, bits / 8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/sys/class/hidraw/hidraw0/device/report_descriptor` on a Legion 5 15AHP11
    /// (`048d:c615`), 231 bytes.
    const ITE_8258: &str = concat!(
        "0689ff0910a101855a0901150026ff0075089510b100c00689ff0907a10185070901150026ff00750896bf03b100c0",
        "0689ff09cca10185cc0901150026ff0075089520b100c005010906a101850175019508050719e029e7150025018102",
        "950175088103050819012905950575019102950175039103050719002aff00150026ff00950c750881000507190029d",
        "f1500250195e075018102c00699ff0910a10185051a00ff2a10ff1600ff2610ff750895098100c0050c0901a1018502",
        "19002a3c021500263c02751095018100c00501090ca10185031500250109c695017501810675078103c0",
    );

    /// `/sys/class/hidraw/hidraw1/device/report_descriptor`, the LampArray
    /// interface, 327 bytes.
    const LAMPARRAY: &str = concat!(
        "05590901a10185010902a1020903150027ffff000075109501b10309040905090609070908150027ffffff7f75209505b103c085020920a1020921150027ffff000075109501b102c085030922a1020921150027ffff000075109501b102092309240925",
        "09270926150027ffffff7f75209505b10209280929092a092b092c092d150026ff0075089506b102c085040950a102090309551500250875089502b1020921150027ffff000075109508b102095109520953095409510952095309540951095209530954",
        "09510952095309540951095209530954095109520953095409510952095309540951095209530954150026ff0075089520b102c085050960a10209551500250875089501b10209610962150027ffff000075109502b1020951095209530954150026ff00",
        "75089504b102c085060970a10209711500250175089501b102c0c0",
    );

    fn unhex(text: &str) -> Vec<u8> {
        assert!(text.len() % 2 == 0, "odd hex length");
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn finds_the_lighting_signature_after_another_collection() {
        let descriptor = unhex(ITE_8258);
        assert_eq!(descriptor.len(), 231);
        // The trap: it is *not* at offset zero on this controller.
        assert_eq!(find_signature(&descriptor), Some(23));
        assert!(!descriptor.starts_with(&SIGNATURE));
    }

    #[test]
    fn reads_the_ite_feature_reports() {
        let reports = feature_reports(&unhex(ITE_8258));
        assert_eq!(reports.get(&0x07).copied(), Some(959));
        assert_eq!(reports.get(&0x5A).copied(), Some(16));
        assert_eq!(reports.get(&0xCC).copied(), Some(32));
        assert_eq!(reports.len(), 3);
    }

    #[test]
    fn reads_the_lamparray_feature_reports() {
        let descriptor = unhex(LAMPARRAY);
        assert_eq!(descriptor.len(), 327);
        let reports = feature_reports(&descriptor);
        assert_eq!(reports.get(&0x01).copied(), Some(22));
        assert_eq!(reports.get(&0x02).copied(), Some(2));
        assert_eq!(reports.get(&0x03).copied(), Some(28));
        assert_eq!(reports.get(&0x04).copied(), Some(50));
        assert_eq!(reports.get(&0x05).copied(), Some(9));
        assert_eq!(reports.get(&0x06).copied(), Some(1));
        // The LampArray interface must never pass the lighting gate.
        assert_eq!(find_signature(&descriptor), None);
    }

    #[test]
    fn truncated_descriptors_do_not_panic() {
        let descriptor = unhex(ITE_8258);
        for end in 0..descriptor.len() {
            let _ = feature_reports(&descriptor[..end]);
            let _ = find_signature(&descriptor[..end]);
        }
    }
}
