//! On its own, as a process of its own: it lowers how many files the
//! process may hold open, which every other test would feel.

#![cfg(unix)]

mod common;

use std::sync::atomic::AtomicBool;

use common::{MIB, machine, write};
use harbormaster_cleaner::{Hunt, large};

#[test]
fn finds_every_large_file_with_few_files_open_at_once() {
    let limit = libc::rlimit {
        rlim_cur: 64,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: `limit` is a valid rlimit; lowering the soft limit is always allowed.
    let lowered = unsafe {
        let mut now = std::mem::zeroed::<libc::rlimit>();
        libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut now) == 0
            && libc::setrlimit(
                libc::RLIMIT_NOFILE,
                &libc::rlimit {
                    rlim_max: now.rlim_max,
                    ..limit
                },
            ) == 0
    };
    assert!(lowered, "the limit should be lowered");
    let machine = machine();
    for top in ["a", "b", "c", "d"] {
        let deep = (0..31).fold(machine.home.join(top), |path, level| {
            path.join(format!("l{level}"))
        });
        write(&deep.join("big.mov"), &vec![1; 2 * MIB]);
    }
    let hunt = Hunt {
        home: &machine.home,
        smallest: MIB as u64,
        busy: &[],
    };

    let found = large(&hunt, &AtomicBool::new(false), &|_| {}).expect("the hunt should start");

    assert_eq!(found.len(), 4, "{found:?}");
}
