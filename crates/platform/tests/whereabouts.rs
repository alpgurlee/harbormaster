#![cfg(target_os = "macos")]

use std::fs::File;

#[test]
fn finds_where_this_process_is() {
    let place = tempfile::tempdir().unwrap();
    let held = place.path().canonicalize().unwrap().join("held");
    let _open = File::create(&held).unwrap();
    let here = std::env::current_dir().unwrap().canonicalize().unwrap();
    let own = std::process::id();

    let found = harbormaster_platform::whereabouts().unwrap();

    let mine = found
        .iter()
        .find(|each| each.key.pid.0 == own)
        .expect("this process should be listed");
    assert!(mine.paths.contains(&held), "{:?}", mine.paths);
    assert!(mine.paths.contains(&here), "{:?}", mine.paths);
    let program = std::env::current_exe().unwrap().canonicalize().unwrap();
    assert!(mine.paths.contains(&program), "{:?}", mine.paths);
    assert!(!mine.name.is_empty());
}
