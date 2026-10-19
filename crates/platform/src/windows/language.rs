use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// The first display language the user chose, which the app's web view
/// follows too.
pub(crate) fn preferred_language() -> Option<String> {
    let (mut count, mut length) = (0, 0);
    // SAFETY: a null buffer asks only for the length needed.
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut length,
        )
    };
    if sized == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: the buffer holds `length` UTF-16 units.
    let read = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if read == 0 {
        return None;
    }
    first(&buffer)
}

/// The first of a list of names, each ended by a nul.
fn first(list: &[u16]) -> Option<String> {
    let name = list.split(|&unit| unit == 0).next()?;
    (!name.is_empty()).then(|| String::from_utf16_lossy(name))
}

#[cfg(test)]
mod tests {
    use super::first;

    #[test]
    fn takes_the_first_of_the_languages() {
        let list: Vec<u16> = "tr-TR\0en-US\0\0".encode_utf16().collect();

        assert_eq!(first(&list).as_deref(), Some("tr-TR"));
        assert_eq!(first(&[0, 0]), None);
    }
}
