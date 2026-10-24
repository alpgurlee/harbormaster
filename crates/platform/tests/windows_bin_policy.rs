#![cfg(windows)]
//! With group policy turning this user's Recycle Bin off, or leaving it no
//! room, whatever the user set.

mod recycle_bin;

use recycle_bin::{POLICIES, Setting, left_where_it_was, place, take_turn};

#[test]
fn leaves_where_it_was_what_policy_keeps_out_of_the_recycle_bin() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, b"a film").unwrap();
    let _off = Setting::changed(POLICIES, "NoRecycleFiles", 1);

    left_where_it_was(&file);
}

#[test]
fn leaves_where_it_was_what_is_larger_than_policy_lets_the_recycle_bin_be() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, b"a film").unwrap();
    let _none = Setting::changed(POLICIES, "RecycleBinSize", 0);

    left_where_it_was(&file);
}
