//! The languages the user prefers, as macOS lists them.

use objc2::rc::autoreleasepool;
use objc2_foundation::NSLocale;

/// Safe from any thread, with an autorelease pool of its own.
pub(crate) fn preferred_language() -> Option<String> {
    autoreleasepool(|_| {
        NSLocale::preferredLanguages()
            .firstObject()
            .map(|tag| tag.to_string())
    })
}
