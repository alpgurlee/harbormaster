//! The Trash, through Foundation, so that Finder can put an item back.

use std::io;
use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSFileManager, NSString, NSURL};

pub(crate) fn trash(path: &Path) -> io::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path isn't valid text"))?;
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        let mut landed = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
            .map_err(|error| io::Error::other(error.localizedDescription().to_string()))?;
        landed
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
            .ok_or_else(|| io::Error::other("the Trash didn't say where it put the item"))
    })
}
