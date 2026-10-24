#![doc = include_str!("../README.md")]

#[cfg(feature = "csgo")]
pub mod csgo {
    include!(concat!(env!("OUT_DIR"), "/csgo/csgo.rs"));
}

#[cfg(feature = "deadlock")]
pub mod deadlock {
    include!(concat!(env!("OUT_DIR"), "/deadlock/deadlock.rs"));
}

#[cfg(feature = "dota2")]
pub mod dota2 {
    include!(concat!(env!("OUT_DIR"), "/dota2/dota2.rs"));
}

pub mod gc {
    include!(concat!(env!("OUT_DIR"), "/gc/gc.rs"));
}

pub mod steam {
    include!(concat!(env!("OUT_DIR"), "/steam/steam.rs"));
}

#[cfg(feature = "tf2")]
pub mod tf2 {
    include!(concat!(env!("OUT_DIR"), "/tf2/tf2.rs"));
}

#[cfg(feature = "webui")]
pub mod webui {
    include!(concat!(env!("OUT_DIR"), "/webui/webui.rs"));
}
