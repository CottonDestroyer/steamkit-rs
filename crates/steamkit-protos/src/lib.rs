//! Steam protobuf messages and enums, generated with `prost` from Valve's
//! `.proto` files.
//!
//! | Module | Contents |
//! |---|---|
//! | [`steam`] | Steam client messages |
//! | [`webui`] | Web UI service messages |
//! | [`csgo`] | Counter-Strike game coordinator |
//! | [`dota2`] | Dota 2 game coordinator |
//! | [`tf2`] | Team Fortress 2 game coordinator |
//! | [`deadlock`] | Deadlock game coordinator |
//! | [`gc`] | Shared game coordinator messages |
//!
//! The proto files don't declare a package, so every file in a directory
//! ends up in the same module.

pub mod csgo {
    include!(concat!(env!("OUT_DIR"), "/csgo/csgo.rs"));
}

pub mod deadlock {
    include!(concat!(env!("OUT_DIR"), "/deadlock/deadlock.rs"));
}

pub mod dota2 {
    include!(concat!(env!("OUT_DIR"), "/dota2/dota2.rs"));
}

pub mod gc {
    include!(concat!(env!("OUT_DIR"), "/gc/gc.rs"));
}

pub mod steam {
    include!(concat!(env!("OUT_DIR"), "/steam/steam.rs"));
}

pub mod tf2 {
    include!(concat!(env!("OUT_DIR"), "/tf2/tf2.rs"));
}

pub mod webui {
    include!(concat!(env!("OUT_DIR"), "/webui/webui.rs"));
}
