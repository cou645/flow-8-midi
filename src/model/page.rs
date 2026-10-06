use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    DeviceSelect,
    Mixer,
    MixerFx,
    Eq,
    Sends,
    Fx,
    Snapshots,
    Routing,
    Settings,
}

impl fmt::Display for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match *self {
            Page::DeviceSelect => "Device",
            Page::Mixer => "Mixer",
            Page::MixerFx => "Mixer+",
            Page::Eq => "EQ",
            Page::Sends => "Sends",
            Page::Fx => "FX",
            Page::Snapshots => "Snapshots",
            Page::Routing => "Routing",
            Page::Settings => "Settings",
        };
        write!(f, "{}", text)
    }
}
