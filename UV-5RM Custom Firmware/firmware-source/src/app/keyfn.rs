#[cfg(feature = "ar5rm-nrf")]
use super::overlay;
use super::{scan, App};
use cortex_m::peripheral::SYST;

pub(super) const POWER_CODE: u8 = 7;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyFunction {
    None,
    WideNarrow,
    Monitor,
    Mode,
    TxTone,
    /// Opens the installed app named "FM" (broadcast radio is an app now).
    #[cfg(feature = "ar5rm-nrf")]
    FmRadio,
    #[cfg(feature = "ar5rm-nrf")]
    DayNight,
    Scan,
    Power,
    Flashlight,
    Reverse,
}

impl KeyFunction {
    pub fn label(self) -> &'static str {
        match self {
            KeyFunction::None => "NONE",
            KeyFunction::WideNarrow => "WIDE/NAR",
            KeyFunction::Monitor => "MONITOR",
            KeyFunction::Mode => "MODE",
            KeyFunction::TxTone => "TX TONE",
            #[cfg(feature = "ar5rm-nrf")]
            KeyFunction::FmRadio => "FM RADIO",
            #[cfg(feature = "ar5rm-nrf")]
            KeyFunction::DayNight => "DAY/NIGHT",
            KeyFunction::Scan => "SCAN",
            KeyFunction::Power => "POWER",
            KeyFunction::Flashlight => "LIGHT",
            KeyFunction::Reverse => "REVERSE",
        }
    }
}

pub(super) fn from_u8(v: u8) -> KeyFunction {
    match v {
        1 => KeyFunction::WideNarrow,
        2 => KeyFunction::Monitor,
        3 => KeyFunction::Mode,
        4 => KeyFunction::TxTone,
        #[cfg(feature = "ar5rm-nrf")]
        5 => KeyFunction::FmRadio,
        6 => KeyFunction::Scan,
        POWER_CODE => KeyFunction::Power,
        8 => KeyFunction::Flashlight,
        // 9 => search (now app)
        10 => KeyFunction::Reverse,
        #[cfg(feature = "ar5rm-nrf")]
        11 => KeyFunction::DayNight,
        _ => KeyFunction::None,
    }
}

#[cfg_attr(feature = "ar5rm-nrf", inline(never))]
pub(super) fn invoke(app: &mut App, syst: &mut SYST, func: KeyFunction) {
    match func {
        KeyFunction::None | KeyFunction::TxTone => {}
        KeyFunction::WideNarrow => app.toggle_wide_narrow(syst),
        KeyFunction::Monitor => app.radio.toggle_monitor(),
        KeyFunction::Mode => app.toggle_modulation(syst),
        // Does nothing when no FM app is installed. The UV-5RM image has no
        // flash left for this yet, so there 5 still means NONE.
        #[cfg(feature = "ar5rm-nrf")]
        KeyFunction::FmRadio => overlay::enter_named(app, syst, "FM"),
        #[cfg(feature = "ar5rm-nrf")]
        KeyFunction::DayNight => {
            if app.settings.theme == 3 {
                app.settings.theme = app.settings.dark_theme.min(2);
            } else {
                app.settings.dark_theme = app.settings.theme.min(2);
                app.settings.theme = 3;
            }
            app.save_settings();
        }
        KeyFunction::Scan => scan::enter(app, syst),
        KeyFunction::Power => app.toggle_power(syst),
        KeyFunction::Flashlight => app.flashlight.toggle(),
        // TODO: search
        KeyFunction::Reverse => app.toggle_reverse(syst),
    }
}
