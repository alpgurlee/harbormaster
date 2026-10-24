#![cfg(windows)]
//! A process of its own: the test empties this user's Recycle Bins, which
//! any test moving things there would meet.

mod common;

use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, MIB, machine, reach, write};
use harbormaster_cleaner::{
    Group, Kind, Outcome, Survey, Usage, Why, is_recycle_bin, recheck, remove, scan,
};

#[test]
fn lists_what_the_recycle_bin_holds_and_empties_it_for_good() {
    let machine = machine();
    let film = machine.home.join(r"Videos\trip.mp4");
    write(&film, &vec![1; 2 * MIB]);
    let landed = harbormaster_platform::trash(&film).expect("moved to the Recycle Bin");
    let survey = Survey {
        home: &machine.home,
        roots: &[],
        busy: &[],
        running: &[],
        idle_after: 30 * DAY,
        now: SystemTime::now(),
    };

    let findings = scan(&survey, &AtomicBool::new(false)).expect("the scan should start");

    let bin = findings
        .iter()
        .find(|finding| finding.group == Group::Trash)
        .expect("the Recycle Bin");
    assert_eq!(
        (bin.kind, bin.picked, bin.emptied),
        (Kind::Discarded, false, true)
    );
    assert!(bin.bytes >= 2 * MIB as u64, "{bin:?}");
    assert!(is_recycle_bin(&bin.path), "{}", bin.path.display());

    // Something thrown away after the scan keeps the bins from being emptied.
    let later = machine.home.join(r"Videos\later.mp4");
    write(&later, &vec![1; 2 * MIB]);
    let thrown = harbormaster_platform::trash(&later).expect("moved to the Recycle Bin");
    let refused = remove(
        &reach(&machine.home),
        std::slice::from_ref(bin),
        &|| Ok(Usage::default()),
        &|_| panic!("nothing goes to the Recycle Bin"),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");
    assert_eq!(
        refused.removals[0].outcome,
        Outcome::Refused { why: Why::Changed },
        "{refused:?}"
    );
    assert!(landed.exists() && thrown.exists());

    // Looked at again, the bins go with all they hold.
    let findings = scan(&survey, &AtomicBool::new(false)).expect("the scan should start");
    let bin = findings
        .iter()
        .find(|finding| finding.group == Group::Trash)
        .expect("the Recycle Bin");
    let cleaned = remove(
        &reach(&machine.home),
        std::slice::from_ref(bin),
        &|| Ok(Usage::default()),
        &|_| panic!("nothing goes to the Recycle Bin"),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert_eq!(cleaned.removals[0].outcome, Outcome::Deleted, "{cleaned:?}");
    assert!(!landed.exists() && !thrown.exists());
    let now = recheck(
        &reach(&machine.home),
        std::slice::from_ref(bin),
        &Usage::default(),
    )
    .expect("the recheck should run");
    assert_eq!(now, []);
}
