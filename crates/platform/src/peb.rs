//! Where a Windows process keeps its parameters, in its own memory, as read
//! from outside: the places in its PEB and in the parameters below it, for a
//! 64-bit process and for a 32-bit one.

/// The layout a process's memory follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Bits64,
    Bits32,
}

impl Layout {
    fn pointer_size(self) -> usize {
        match self {
            Layout::Bits64 => 8,
            Layout::Bits32 => 4,
        }
    }

    /// Where the PEB at `peb` keeps the pointer to the process parameters.
    pub(crate) fn parameters_at(self, peb: u64) -> Option<u64> {
        peb.checked_add(match self {
            Layout::Bits64 => 0x20,
            Layout::Bits32 => 0x10,
        })
    }

    /// Where the parameters at `parameters` keep the working folder's text.
    pub(crate) fn working_folder_at(self, parameters: u64) -> Option<u64> {
        parameters.checked_add(match self {
            Layout::Bits64 => 0x38,
            Layout::Bits32 => 0x24,
        })
    }

    /// How large a text descriptor is: a length, a capacity and a pointer.
    pub(crate) fn descriptor_size(self) -> usize {
        2 * self.pointer_size()
    }
}

/// Where a text is and how long: a `UNICODE_STRING`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Text {
    /// Its length in bytes, without any nul.
    pub(crate) bytes: usize,
    pub(crate) at: u64,
}

/// The pointer `image` holds at `at`.
pub(crate) fn pointer_in(image: &[u8], at: usize, layout: Layout) -> Option<u64> {
    let raw = image.get(at..at.checked_add(layout.pointer_size())?)?;
    Some(match layout {
        Layout::Bits64 => u64::from_le_bytes(raw.try_into().ok()?),
        Layout::Bits32 => u64::from(u32::from_le_bytes(raw.try_into().ok()?)),
    })
}

/// The text descriptor `image` holds at `at`: its length, then its
/// capacity, then, aligned as a pointer, where the text is.
pub(crate) fn descriptor_in(image: &[u8], at: usize, layout: Layout) -> Option<Text> {
    let length = image.get(at..at.checked_add(2)?)?;
    let bytes = usize::from(u16::from_le_bytes(length.try_into().ok()?));
    let pointer = pointer_in(image, at.checked_add(layout.pointer_size())?, layout)?;
    Some(Text { bytes, at: pointer })
}

/// The UTF-16 text of `bytes` bytes that `image` holds at `at`, when all of
/// it is there.
pub(crate) fn text_in(image: &[u8], at: usize, bytes: usize) -> Option<String> {
    if !bytes.is_multiple_of(2) {
        return None;
    }
    let raw = image.get(at..at.checked_add(bytes)?)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::{Layout, Text, descriptor_in, pointer_in, text_in};

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_a_pointer_of_either_width_and_none_past_the_end() {
        let image = [0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0xAA];

        assert_eq!(
            pointer_in(&image, 0, Layout::Bits64),
            Some(0x1122_3344_5566_7788)
        );
        assert_eq!(pointer_in(&image, 0, Layout::Bits32), Some(0x5566_7788));
        assert_eq!(
            pointer_in(&image, 1, Layout::Bits64),
            Some(0xAA11_2233_4455_6677)
        );
        assert_eq!(pointer_in(&image, 2, Layout::Bits64), None);
        assert_eq!(pointer_in(&image, usize::MAX, Layout::Bits32), None);
    }

    #[test]
    fn reads_a_text_descriptor_as_each_layout_lays_it_out() {
        // 64-bit: length 10, capacity 12, 4 bytes of padding, then the pointer.
        let mut wide = vec![10, 0, 12, 0, 0, 0, 0, 0];
        wide.extend(0x7FF6_1234_0000_u64.to_le_bytes());
        assert_eq!(
            descriptor_in(&wide, 0, Layout::Bits64),
            Some(Text {
                bytes: 10,
                at: 0x7FF6_1234_0000
            })
        );
        // 32-bit: length, capacity, then the pointer at once.
        let mut narrow = vec![10, 0, 12, 0];
        narrow.extend(0x0040_1000_u32.to_le_bytes());
        assert_eq!(
            descriptor_in(&narrow, 0, Layout::Bits32),
            Some(Text {
                bytes: 10,
                at: 0x0040_1000
            })
        );
        assert_eq!(descriptor_in(&narrow, 0, Layout::Bits64), None);
        assert_eq!(Layout::Bits64.descriptor_size(), 16);
        assert_eq!(Layout::Bits32.descriptor_size(), 8);
    }

    #[test]
    fn places_the_parameters_and_the_working_folder_without_wrapping_around() {
        assert_eq!(Layout::Bits64.parameters_at(0x1000), Some(0x1020));
        assert_eq!(Layout::Bits32.parameters_at(0x1000), Some(0x1010));
        assert_eq!(Layout::Bits64.working_folder_at(0x2000), Some(0x2038));
        assert_eq!(Layout::Bits32.working_folder_at(0x2000), Some(0x2024));
        // A process may put anything where its parameters pointer should be.
        assert_eq!(Layout::Bits64.parameters_at(u64::MAX), None);
        assert_eq!(Layout::Bits32.working_folder_at(u64::MAX - 1), None);
    }

    #[test]
    fn reads_a_text_only_when_all_of_it_is_there() {
        let mut image = vec![0xFF; 4];
        image.extend(utf16("C:\\Users\\dev\\"));

        assert_eq!(text_in(&image, 4, 26).as_deref(), Some("C:\\Users\\dev\\"));
        assert_eq!(text_in(&image, 4, 28), None, "past the end");
        assert_eq!(text_in(&image, 4, 7), None, "half a character");
        assert_eq!(text_in(&image, 4, 0).as_deref(), Some(""));
    }
}
