use std::time::Duration;

use anyhow::anyhow;
use dbus::{
    blocking::{BlockingSender, Connection},
    message::Message,
    strings::{BusName, Interface, Member},
    Path,
};

use crate::anyhow_map;

/// Bit flags that are used in the DBus API
#[allow(nonstandard_style)]
pub mod InhibitFlags {
    pub const LogOut: u32 = 1;
    pub const SwitchUser: u32 = 2;
    pub const Suspend: u32 = 4;
    pub const Idle: u32 = 8;
    pub const AutoMount: u32 = 16;
}

/// Guard that will keep sleep inhibited until it is dropped
pub struct GnomeInhibitGuard {
    _conn: Connection,
}

impl GnomeInhibitGuard {
    pub fn new(conn: Connection) -> Self {
        Self { _conn: conn }
    }
}

pub fn inhibit_sleep() -> anyhow::Result<GnomeInhibitGuard> {
    let conn = Connection::new_session()?;
    let bus = BusName::new("org.gnome.SessionManager").map_err(anyhow_map)?;
    let path = Path::new("/org/gnome/SessionManager").map_err(anyhow_map)?;
    let iface = Interface::new("org.gnome.SessionManager").map_err(anyhow_map)?;
    let method = Member::new("Inhibit").map_err(anyhow_map)?;
    let flags = InhibitFlags::Suspend | InhibitFlags::Idle;
    // Arguments to the `Inhibit` method. the second argument `0u32` is supposed to be the X window
    // identifier of the blocking program, but it is set to zero here as there should never be an X
    // window associated with our program.
    let args = ("block-sleep", 0u32, "Sleep block manually requested", flags);
    let msg = Message::call_with_args(bus, path, iface, method, args);
    let _ = conn
        .send_with_reply_and_block(msg, Duration::from_secs(3))
        .map_err(|e| anyhow!(e))?;
    Ok(GnomeInhibitGuard::new(conn))
}
