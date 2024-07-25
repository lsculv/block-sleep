use std::ops::BitOr;

use anyhow::anyhow;
use zbus::blocking::{Connection, Proxy};

/// Bit flags that are used in the DBus API
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum InhibitFlags {
    LogOut = 1,
    SwitchUser = 2,
    Suspend = 4,
    Idle = 8,
    AutoMount = 16,
}

impl BitOr for InhibitFlags {
    type Output = u32;

    fn bitor(self, rhs: Self) -> Self::Output {
        // SAFETY: `InhibitFlags` is `repr(u32)` so this conversion is always valid
        let lhs: u32 = unsafe { std::mem::transmute(self) };
        let rhs: u32 = unsafe { std::mem::transmute(rhs) };
        lhs | rhs
    }
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
    let conn = Connection::session()?;

    let destination = "org.gnome.SessionManager";
    let path = "/org/gnome/SessionManager";
    let interface = "org.gnome.SessionManager";

    let proxy = Proxy::new(&conn, destination, path, interface)?;
    let flags = InhibitFlags::Suspend | InhibitFlags::Idle;
    // Args are: in  -> `App ID`, `X Window ID`, `Reason`, `Inhibit Flags`
    //           out -> `Inhibit Cookie`
    let args = ("block-sleep", 0u32, "Sleep block manually requested", flags);

    let result = proxy.call::<&str, (&str, u32, &str, u32), u32>("Inhibit", &args);
    if let Err(e) = result {
        return Err(anyhow!(
            "Failed to block sleep using the Gnome session manager: {}",
            e
        ));
    }

    Ok(GnomeInhibitGuard::new(conn))
}
