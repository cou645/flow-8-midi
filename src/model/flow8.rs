use std::collections::HashMap;
use std::fmt;
use std::ops::RangeInclusive;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use iced::Theme;
use midir::{MidiInputConnection, MidiOutputConnection};

use super::channels::{
    AudioConnection, Bus, BusType, Channel, ChannelType, FxSlot, PhantomPower, PhantomPowerType,
};
use super::page::Page;
use crate::service::ble::{BleConnection, BleStatus};
#[cfg(any(debug_assertions, feature = "dev-tools"))]
use crate::service::sysex_calibration::CalibrationState;

pub const SNAPSHOT_COUNT: usize = 15;
/// 48V needs a second click within this window (was 500 ms — too tight for touch/stylus).
pub const PHANTOM_CONFIRM_MS: u128 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncInterval {
    Never,
    Sec1,
    Secs5,
    Secs30,
    Min1,
    Min2,
    Min5,
    Min10,
    Min15,
}

impl SyncInterval {
    pub const ALL: &'static [SyncInterval] = &[
        SyncInterval::Never,
        SyncInterval::Sec1,
        SyncInterval::Secs5,
        SyncInterval::Secs30,
        SyncInterval::Min1,
        SyncInterval::Min2,
        SyncInterval::Min5,
        SyncInterval::Min10,
        SyncInterval::Min15,
    ];

    pub fn as_secs(&self) -> Option<u64> {
        match self {
            SyncInterval::Never => None,
            SyncInterval::Sec1 => Some(1),
            SyncInterval::Secs5 => Some(5),
            SyncInterval::Secs30 => Some(30),
            SyncInterval::Min1 => Some(60),
            SyncInterval::Min2 => Some(120),
            SyncInterval::Min5 => Some(300),
            SyncInterval::Min10 => Some(600),
            SyncInterval::Min15 => Some(900),
        }
    }
}

impl fmt::Display for SyncInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncInterval::Never => write!(f, "Never"),
            SyncInterval::Sec1 => write!(f, "1s"),
            SyncInterval::Secs5 => write!(f, "5s"),
            SyncInterval::Secs30 => write!(f, "30s"),
            SyncInterval::Min1 => write!(f, "1 min"),
            SyncInterval::Min2 => write!(f, "2 min"),
            SyncInterval::Min5 => write!(f, "5 min"),
            SyncInterval::Min10 => write!(f, "10 min"),
            SyncInterval::Min15 => write!(f, "15 min"),
        }
    }
}

pub struct FLOW8Controller {
    pub current_page: Page,
    pub theme: Theme,
    pub midi_conn: Option<MidiOutputConnection>,
    pub midi_input_conn: Option<MidiInputConnection<()>>,
    pub connected_device_name: Option<String>,
    pub connection_error: Option<String>,
    pub channels: Vec<Channel>,
    pub buses: Vec<Bus>,
    pub fx_slots: Vec<FxSlot>,
    pub sysex_receiver: Option<mpsc::Receiver<Vec<u8>>>,
    pub sysex_sender: Option<mpsc::Sender<Vec<u8>>>,
    pub last_sysex_dump: Option<Vec<u8>>,
    pub ble_status: BleStatus,
    pub ble_status_receiver: Option<mpsc::Receiver<BleStatus>>,
    pub ble_available: bool,
    pub ble_connection: Arc<Mutex<Option<BleConnection>>>,
    pub tick_counter: u32,
    pub ble_last_click: Option<Instant>,
    pub sync_last_click: Option<Instant>,
    pub sync_interval: SyncInterval,
    pub last_sync_time: Option<Instant>,
    pub snapshot_names: Vec<Option<String>>,
    pub snapshot_names_receiver: Option<mpsc::Receiver<Vec<Option<String>>>>,
    pub fx_muted: bool,
    pub snapshot_resync_at: Option<Instant>,
    /// Phones level in dB from the SysEx dump (read-only; no known write command).
    pub phones_db: Option<f32>,
    /// PHONES slider position (0..=255); follows the dump unless dragged in the last 2 s.
    pub phones_value: Option<u8>,
    pub phones_touched: Option<Instant>,
    /// Live input meters from the BLE 0x22 stream (see routing::meter_slots).
    pub meters: [u8; 12],
    pub meters_at: Option<Instant>,
    pub meters_receiver: Option<mpsc::Receiver<[u8; 12]>>,
    /// BLE settings (model::routing::SETTINGS) by id, as last reported/set.
    pub settings: HashMap<u8, u8>,
    pub settings_receiver: Option<mpsc::Receiver<(u8, u8)>>,
    /// FX1/FX2 return routing masks; None until set from here (no known read).
    pub fx_routes: [Option<u8>; 2],
    #[cfg(any(debug_assertions, feature = "dev-tools"))]
    pub calibration: CalibrationState,
}

const CHANNEL_RANGE: RangeInclusive<u8> = 0..=6;
const BUS_RANGE: RangeInclusive<u8> = 7..=11;

impl FLOW8Controller {
    /// Meter levels for one input strip; zeros once the stream has been quiet for 2 s.
    pub fn channel_meters(&self, channel_id: u8) -> Vec<u8> {
        let live = self.meters_at.is_some_and(|t| t.elapsed().as_secs() < 2);
        crate::model::routing::meter_slots(channel_id)
            .iter()
            .map(|&i| if live { self.meters[i] } else { 0 })
            .collect()
    }

    pub fn mark_all_synced(&mut self) {
        for ch in &mut self.channels {
            ch.mark_all_synced();
        }
        for bus in &mut self.buses {
            bus.mark_all_synced();
        }
        for fx in &mut self.fx_slots {
            fx.mark_all_synced();
        }
    }

    pub fn mark_all_unsynced(&mut self) {
        for ch in &mut self.channels {
            ch.mark_all_unsynced();
        }
        for bus in &mut self.buses {
            bus.mark_all_unsynced();
        }
        for fx in &mut self.fx_slots {
            fx.mark_all_unsynced();
        }
    }

    pub fn is_globally_synced(&self) -> bool {
        self.channels.iter().all(|c| c.is_all_synced())
            && self.buses.iter().all(|b| b.is_all_synced())
            && self.fx_slots.iter().all(|f| f.is_all_synced())
    }

    pub fn new() -> FLOW8Controller {
        FLOW8Controller {
            current_page: Page::DeviceSelect,
            theme: Theme::Dark,
            midi_conn: None,
            midi_input_conn: None,
            connected_device_name: None,
            connection_error: None,
            channels: CHANNEL_RANGE
                .map(|c_id| Channel {
                    id: c_id,
                    phantom_pwr: PhantomPower {
                        is_on: false,
                        phantom_power_type: match c_id {
                            0..=1 => PhantomPowerType::Set48v,
                            _ => PhantomPowerType::None,
                        },
                    },
                    channel_type: match c_id {
                        0..=3 => ChannelType::Mono,
                        _ => ChannelType::Stereo,
                    },
                    audio_connection: match c_id {
                        0..=1 => AudioConnection::Xlr,
                        2..=3 => AudioConnection::ComboXlr,
                        4..=5 => AudioConnection::Line,
                        _ => AudioConnection::UsbBt,
                    },
                    ..Default::default()
                })
                .collect(),
            buses: BUS_RANGE
                .map(|b_id| Bus {
                    id: b_id,
                    index: b_id - *BUS_RANGE.start(),
                    bus_type: match b_id {
                        7 => BusType::Main,
                        8..=9 => BusType::Monitor,
                        _ => BusType::Fx,
                    },
                    ..Default::default()
                })
                .collect(),
            fx_slots: vec![FxSlot::new(0), FxSlot::new(1)],
            sysex_receiver: None,
            sysex_sender: None,
            last_sysex_dump: None,
            ble_status: BleStatus::Unavailable,
            ble_status_receiver: None,
            ble_available: false,
            ble_connection: Arc::new(Mutex::new(None)),
            tick_counter: 0,
            ble_last_click: None,
            sync_last_click: None,
            sync_interval: SyncInterval::Sec1,
            last_sync_time: None,
            snapshot_names: vec![None; SNAPSHOT_COUNT],
            snapshot_names_receiver: None,
            fx_muted: false,
            snapshot_resync_at: None,
            phones_db: None,
            phones_value: None,
            phones_touched: None,
            meters: [0; 12],
            meters_at: None,
            meters_receiver: None,
            settings: HashMap::new(),
            settings_receiver: None,
            fx_routes: [None; 2],
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            calibration: CalibrationState::new(),
        }
    }
}
