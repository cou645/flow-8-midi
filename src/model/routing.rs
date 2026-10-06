//! Mixer settings the FLOW app sets over BLE (not reachable via MIDI).
//!
//! Get/set use the generic setting record `25 01 id len value… chk` (set, and the
//! mixer's reply) / `26 01 id chk` (get). IDs and values were taken from an HCI capture
//! of FLOW app 1.12 and checked against its Routing / Preferences screens — see
//! docs/flow8-midi-implementation.md §3.5b.

pub struct Setting {
    pub section: &'static str,
    pub label: &'static str,
    pub id: u8,
    pub options: &'static [(&'static str, u8)],
}

const OFF_ON: &[(&str, u8)] = &[("Off", 0), ("On", 1)];
const PRE_POST: &[(&str, u8)] = &[("Pre", 0), ("Post", 1)];

pub const SETTINGS: &[Setting] = &[
    Setting { section: "USB", label: "Output mode", id: 0x07, options: &[("Recording", 0), ("Streaming", 1)] },
    Setting { section: "Phones", label: "Source", id: 0x0C, options: &[("Main", 0), ("Mon 1/2", 1)] },
    Setting { section: "Phones", label: "Tap point", id: 0x0D, options: PRE_POST },
    Setting { section: "Monitor 1/2", label: "Tap point", id: 0x11, options: PRE_POST },
    Setting { section: "Monitor 1/2", label: "Stereo link", id: 0x0B, options: OFF_ON },
    Setting { section: "BT/USB play", label: "Destination", id: 0x02, options: &[("To main mix", 0), ("Phones only", 1)] },
    Setting { section: "From PC (USB)", label: "USB left", id: 0x08, options: &[("Off", 0), ("To channel", 1), ("Mon out 1/2", 2)] },
    Setting { section: "From PC (USB)", label: "USB right → channel", id: 0x09, options: OFF_ON },
    Setting { section: "From PC (USB)", label: "USB right → Mon out 1/2", id: 0x0A, options: OFF_ON },
    Setting { section: "Preferences", label: "Foot switch mode", id: 0x03, options: &[("FX mute/tap", 0), ("Snapshot up/down", 1)] },
    Setting { section: "Preferences", label: "-10 dBV Main out", id: 0x0E, options: OFF_ON },
    Setting { section: "Preferences", label: "-10 dBV Monitor out", id: 0x0F, options: OFF_ON },
    Setting { section: "Preferences", label: "Mixer app link", id: 0xB0, options: &[("Independent", 0), ("Layers follow", 1)] },
];

/// FX return routing: `11 01 bus x 00 00 mask chk`, one 3-bit mask per FX bus.
/// (bus, x) pairs exactly as the FLOW app sends them; x's meaning is unknown.
pub const FX_ROUTE_BUS: [(u8, u8); 2] = [(0x0C, 0x18), (0x0D, 0x17)];
pub const FX_ROUTE_DESTS: [(&str, u8); 3] = [("To Main", 0b100), ("To Mon 1", 0b010), ("To Mon 2", 0b001)];

pub fn with_checksum(body: &[u8]) -> Vec<u8> {
    let mut p = body.to_vec();
    p.push(body.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    p
}

pub fn set_packet(id: u8, value: u8) -> Vec<u8> {
    with_checksum(&[0x25, 0x01, id, 0x01, value])
}

pub fn get_packet(id: u8) -> Vec<u8> {
    with_checksum(&[0x26, 0x01, id])
}

pub fn fx_route_packet(fx: usize, mask: u8) -> Vec<u8> {
    let (bus, x) = FX_ROUTE_BUS[fx];
    with_checksum(&[0x11, 0x01, bus, x, 0x00, 0x00, mask])
}

/// `25 01 id len value… chk` → (id, first value byte) for 1-byte settings.
pub fn parse_setting_reply(p: &[u8]) -> Option<(u8, u8)> {
    match p {
        [0x25, 0x01, id, 0x01, value, _chk] => Some((*id, *value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Packets verbatim from the FLOW app HCI capture (captures/session-full.btsnoop).
    #[test]
    fn packets_match_capture() {
        assert_eq!(set_packet(0x07, 0x01), [0x25, 0x01, 0x07, 0x01, 0x01, 0x2F]);
        assert_eq!(set_packet(0xB0, 0x00), [0x25, 0x01, 0xB0, 0x01, 0x00, 0xD7]);
        assert_eq!(get_packet(0xB0), [0x26, 0x01, 0xB0, 0xD7]);
        assert_eq!(fx_route_packet(0, 0x03), [0x11, 0x01, 0x0C, 0x18, 0x00, 0x00, 0x03, 0x39]);
        assert_eq!(fx_route_packet(1, 0x07), [0x11, 0x01, 0x0D, 0x17, 0x00, 0x00, 0x07, 0x3D]);
        assert_eq!(parse_setting_reply(&[0x25, 0x01, 0x0B, 0x01, 0x01, 0x33]), Some((0x0B, 1)));
        assert_eq!(parse_setting_reply(&[0x25, 0x01, 0x80, 0x00, 0xA6]), None);
    }
}
