#![cfg(target_os = "macos")]

use harbormaster_platform::{Thermal, thermal};

#[test]
fn reads_how_hard_the_mac_is_cooling_from_any_thread() {
    let state = std::thread::spawn(thermal)
        .join()
        .expect("reading the thermal state should not panic");

    assert_ne!(state, Thermal::Unknown);
}
