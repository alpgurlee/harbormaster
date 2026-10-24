#![cfg(windows)]
//! With this user's Recycle Bin for the drive set to take 1 MB. The shell
//! says it will recycle a larger file, then deletes it for good.

mod recycle_bin;

use harbormaster_platform::trash;
use recycle_bin::{LARGEST, MIB, Setting, left_where_it_was, place, take_turn};

#[test]
fn leaves_a_file_larger_than_the_recycle_bin_where_it_was() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, vec![1; 2 * MIB]).unwrap();
    let _small = Setting::changed(&file, LARGEST, 1);

    left_where_it_was(&file);
}

#[test]
fn leaves_a_file_a_byte_larger_than_the_recycle_bin_where_it_was() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, vec![1; MIB + 1]).unwrap();
    let _small = Setting::changed(&file, LARGEST, 1);

    left_where_it_was(&file);
}

#[test]
fn moves_a_file_as_large_as_the_recycle_bin_there() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let file = place.join("clip.mov");
    std::fs::write(&file, vec![1; MIB]).unwrap();
    let _small = Setting::changed(&file, LARGEST, 1);

    let landed = trash(&file).unwrap();

    assert!(!file.exists());
    assert_eq!(std::fs::metadata(&landed).unwrap().len(), MIB as u64);
}

#[test]
fn leaves_a_folder_larger_than_the_recycle_bin_whole() {
    let _turn = take_turn();
    let (_folder, place) = place();
    let archive = place.join("2026-09-20");
    std::fs::create_dir(&archive).unwrap();
    std::fs::write(archive.join("a"), vec![1; 2 * MIB]).unwrap();
    std::fs::write(archive.join("b"), vec![1; 2 * MIB]).unwrap();
    let _small = Setting::changed(&archive, LARGEST, 1);

    left_where_it_was(&archive);

    assert!(archive.join("a").exists() && archive.join("b").exists());
}
