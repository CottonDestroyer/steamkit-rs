//! `GameID` tests.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use steamkit::types::game_id::GameID;

/// Hashes a value with the standard library's default hasher.
fn hash_code<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn mod_crc_correct() {
    let game_id = GameID::from_mod(420, "Research and Development");

    assert!(game_id.is_mod());
    assert_eq!(420u32, game_id.app_id());
    assert_eq!(GameID::from_value(10210309621176861092), game_id);

    let game_id2 = GameID::from_mod(215, "hidden");

    assert!(game_id2.is_mod());
    assert_eq!(215u32, game_id2.app_id());
    assert_eq!(GameID::from_value(9826266959967158487), game_id2);
}

#[test]
fn shortcut_crc_correct() {
    let game_id = GameID::from_shortcut(
        "\"C:\\Program Files (x86)\\Git\\mingw64\\bin\\wintoast.exe\"",
        "Git for Windows",
    );

    assert!(game_id.is_shortcut());
    assert_eq!(GameID::from_value(12754778225939316736), game_id);
}

#[test]
fn game_ids_k_hashcode_equality() {
    let game_id1 = GameID::from_app_id(570);
    let game_id2 = GameID::from_app_id(570);
    let game_id3 = GameID::from_mod(570, "test");
    let game_id4 = GameID::from_app_id(440);

    assert_eq!(hash_code(&game_id1), hash_code(&game_id2));
    assert_ne!(hash_code(&game_id1), hash_code(&game_id3));
    assert_ne!(hash_code(&game_id2), hash_code(&game_id3));
    assert_ne!(hash_code(&game_id1), hash_code(&game_id4));
}
