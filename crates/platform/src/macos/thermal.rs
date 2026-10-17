//! How hard macOS is working to cool the Mac down.

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

use crate::Thermal;

/// Safe from any thread; the scanner's has no autorelease pool of its own.
pub(crate) fn thermal() -> Thermal {
    autoreleasepool(|_| from_state(NSProcessInfo::processInfo().thermalState()))
}

fn from_state(state: NSProcessInfoThermalState) -> Thermal {
    match state {
        NSProcessInfoThermalState::Nominal => Thermal::Nominal,
        NSProcessInfoThermalState::Fair => Thermal::Fair,
        NSProcessInfoThermalState::Serious => Thermal::Serious,
        NSProcessInfoThermalState::Critical => Thermal::Critical,
        _ => Thermal::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use objc2_foundation::NSProcessInfoThermalState;

    use super::from_state;
    use crate::Thermal;

    #[test]
    fn reads_each_state_macos_names_and_no_other() {
        assert_eq!(
            from_state(NSProcessInfoThermalState::Nominal),
            Thermal::Nominal
        );
        assert_eq!(from_state(NSProcessInfoThermalState::Fair), Thermal::Fair);
        assert_eq!(
            from_state(NSProcessInfoThermalState::Serious),
            Thermal::Serious
        );
        assert_eq!(
            from_state(NSProcessInfoThermalState::Critical),
            Thermal::Critical
        );
        assert_eq!(from_state(NSProcessInfoThermalState(4)), Thermal::Unknown);
    }
}
