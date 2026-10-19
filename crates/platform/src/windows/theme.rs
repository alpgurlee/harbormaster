use std::mem::size_of;

use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

/// Whether the taskbar is light, as Windows' colour settings keep it.
pub(crate) fn light_taskbar() -> Option<bool> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("SystemUsesLightTheme");
    let mut light = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both names end in a nul, and `light` has room for the DWORD asked for.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut light).cast(),
            &mut size,
        )
    };
    (read == NO_ERROR).then_some(light != 0)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
