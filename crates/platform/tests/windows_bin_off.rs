#![cfg(windows)]
//! With this user's Recycle Bin turned off for the drive.

mod recycle_bin;

use recycle_bin::{REMOVE_AT_ONCE, Setting, left_where_it_was, place, take_turn};

#[test]
fn leaves_where_it_was_what_the_recycle_bin_is_turned_off_for() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, b"a film").unwrap();
    let _off = Setting::of_bin_for(&file, REMOVE_AT_ONCE, 1);

    left_where_it_was(&file);
}
