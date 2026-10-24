#![cfg(windows)]
//! With group policy turning this user's Recycle Bin off, or leaving it no
//! room, whatever the user set.

mod recycle_bin;

use recycle_bin::{POLICIES, Setting, left_where_it_was, place, take_turn, whole_of};

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

#[test]
fn leaves_where_it_was_a_file_larger_than_the_share_of_the_drive_policy_allows() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("disk.img");
    // Just over 1% of the drive, which the bin's own size, a tenth of the
    // first 40 GB and a twentieth of the rest, is well over.
    let size = whole_of(&file) / 100 + 64 * 1024 * 1024;
    std::fs::File::create(&file).unwrap().set_len(size).unwrap();
    let _share = Setting::changed(POLICIES, "RecycleBinSize", 1);

    left_where_it_was(&file);
}
