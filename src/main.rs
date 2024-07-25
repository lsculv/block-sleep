use anyhow::anyhow;
use clap::Parser;
use colored::Colorize;
use std::{env, io::Error, process, sync::mpsc, thread, time::Duration};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

pub mod gnome;

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{} {}", "error:".red().bold(), e);
        process::exit(1);
    }
}

/// Block your system from sleeping for an amount of time, or until a certain process
/// exits.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// The process id to wait on. Sleep will be blocked until this process exits.
    #[arg(short = 'p', long = "pid", group = "pid group")]
    pid: Option<Pid>,
    /// Block sleep until the first process in the group has exited.
    #[arg(short = 'f', long = "first", num_args = 1.., value_name = "PID", group = "pid group")]
    pid_first: Option<Vec<Pid>>,
    /// Block sleep until all the given processes in the group have exited.
    #[arg(short = 'a', long = "all", num_args = 1.., value_name = "PID", group = "pid group")]
    pid_all: Option<Vec<Pid>>,
    /// The amount of time to block sleep for in seconds.
    // Hyphens (negative sign) is allowed here and then blocked in the parser
    // to provide better error messages
    #[arg(short = 't', long = "time", value_parser=parse_duration, allow_hyphen_values = true)]
    time: Option<Duration>,
}

fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    let (secs_in_unit, strip_char) = match s.chars().last() {
        c @ Some('d') => (24.0 * 60.0 * 60.0, c),
        c @ Some('h') => (60.0 * 60.0, c),
        c @ Some('m') => (60.0, c),
        c @ Some('s') => (1.0, c),
        // clap *should* prevent this branch from ever happening
        None => return Err(anyhow!("TIME value was empty")),
        _ => (1.0, None),
    };
    let secs = if let Some(c) = strip_char {
        s.strip_suffix(c)
            .expect("we have ensured the stripped char is a suffix of the string")
            .parse::<f64>()
            .map_err(|_| {
                anyhow!("invalid character in TIME value. Accepted number endings are: s, m, h, d.")
            })?
            * secs_in_unit
    } else {
        s.parse::<f64>().map_err(|_| {
            anyhow!("invalid character in TIME value. Accepted number endings are: s, m, h, d.")
        })? * secs_in_unit
    };
    if secs < 0.0 {
        return Err(anyhow!("TIME value cannot be negative."));
    }
    Ok(Duration::from_secs_f64(secs))
}

#[derive(Debug)]
enum Backend {
    /// Uses Gnome's own inhibitors via the org.gnome.SessionManager.Inhibit DBus API.
    /// Gnome inhibitors will ignore ones set through the logind DBus API, so they have to be
    /// handled specially (sigh)
    /// https://lira.no-ip.org:8443/doc/gnome-session/dbus/gnome-session.html#org.gnome.SessionManager.Inhibit
    /// https://discourse.gnome.org/t/gnome-logout-dialog-ignores-inhibitors/8602
    Gnome,
    /// Uses systemd inhibitors via the org.freedesktop.logind.Manager.Inhibit DBus API
    /// https://systemd.io/INHIBITOR_LOCKS/
    SystemdInhibit,
    /// More blunt, masks the sleep, hibernate, and suspend targets.
    /// This is the fallback generic option that should work for all systemd targets
    /// but it requires root privileges.
    #[allow(dead_code)]
    SystemdMask,
    /// Uses the cocoa API (Unimplemented)
    /// https://developer.apple.com/library/archive/qa/qa1340/_index.html
    MacOS,
}

impl Backend {
    /// Tries to get the sleep inhibiting back end from the system.
    pub fn from_system() -> anyhow::Result<Self> {
        let system = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        if cfg!(target_os = "macos") {
            Ok(Backend::MacOS)
        } else if cfg!(target_os = "linux") {
            if let Some(pid1) = system.process(Pid::from(1)) {
                let pid1_name = pid1.name();
                if pid1_name.trim() != "systemd" {
                    return Err(anyhow!(
                        "Only Linux systems using systemd are supported, found \"{pid1_name}\""
                    ));
                }
            } else {
                // This probably should not happen
                return Err(anyhow!("could not ensure the system is using systemd, only Linux systems using systemd are supported"));
            }
            // Ensure that systemd is being used and run as pid 1
            //let pid1_name = fs::read_to_string("/proc/1/comm").unwrap_or("unknown".to_string());

            // Checking if Gnome is being used or not
            let xdg_desktop = env::var("XDG_CURRENT_DESKTOP");
            let desktop_session = env::var("DESKTOP_SESSION");
            let backend = match (xdg_desktop, desktop_session) {
                (Err(_), Err(_)) => Backend::SystemdInhibit,
                (Ok(xdg), Ok(session)) => {
                    if xdg == "GNOME" || session == "gnome" {
                        Backend::Gnome
                    } else {
                        Backend::SystemdInhibit
                    }
                }
                (Err(_), Ok(session)) => {
                    if session == "gnome" {
                        Backend::Gnome
                    } else {
                        Backend::SystemdInhibit
                    }
                }
                (Ok(xdg), Err(_)) => {
                    if xdg == "GNOME" {
                        Backend::Gnome
                    } else {
                        Backend::SystemdInhibit
                    }
                }
            };
            return Ok(backend);
        } else {
            return Err(anyhow!("Only Linux and MacOS are supported"));
        }
    }
}

pub trait IsRunning {
    fn is_running(&self) -> bool;
}

impl IsRunning for Pid {
    fn is_running(&self) -> bool {
        // SAFETY: This use of FFI is safe.
        let proc_alive = unsafe { libc::kill(self.as_u32() as i32, 0) };
        !(proc_alive != 0 && Error::last_os_error().raw_os_error().unwrap() != libc::EPERM)
    }
}

fn run(args: Args) -> anyhow::Result<()> {
    let backend = Backend::from_system()?;
    if let Some(pid) = args.pid {
        block_sleep_on_pid(pid, args.time, backend)
    } else if let Some(pids) = args.pid_first {
        block_sleep_on_first_pid(&pids, args.time, backend)
    } else if let Some(pids) = args.pid_all {
        block_sleep_on_all_pids(&pids, args.time, backend)
    } else if let Some(time) = args.time {
        block_sleep_for_time(time, backend)
    } else {
        block_sleep_indefinitely(backend)
    }
}

fn anyhow_map(e: String) -> anyhow::Error {
    anyhow!(e)
}

fn block_sleep_for_time(time: Duration, backend: Backend) -> anyhow::Result<()> {
    match backend {
        Backend::Gnome => {
            let _guard = gnome::inhibit_sleep()?;
            println!("Sleep blocked for {time:?}");
            thread::sleep(time);
        }
        _ => unimplemented!("Backends other than Gnome are not implemented"),
    }
    Ok(())
}

fn block_sleep_on_pid(pid: Pid, time: Option<Duration>, backend: Backend) -> anyhow::Result<()> {
    // First check that the pid is actually running before doing any blocking to produce a better
    // error message.
    if !pid.is_running() {
        return Err(anyhow!("No such process with pid {pid} was running"));
    }
    match backend {
        Backend::Gnome => {
            let (sender, receiver) = mpsc::sync_channel(0);
            if let Some(t) = time {
                thread::spawn(move || {
                    thread::sleep(t);
                    let _ = sender.send(());
                });
            }
            let _guard = gnome::inhibit_sleep()?;
            println!("Sleep blocked until pid {pid} exits.");
            loop {
                if receiver.try_recv().is_ok() {
                    println!("Timeout reached before process with pid {pid} could exit.");
                    break;
                }
                if !pid.is_running() {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
        _ => unimplemented!(),
    }
    Ok(())
}

fn block_sleep_on_first_pid(
    pids: &[Pid],
    time: Option<Duration>,
    backend: Backend,
) -> anyhow::Result<()> {
    // Check that all the pids supplied are actually running
    for pid in pids {
        if !pid.is_running() {
            return Err(anyhow!("No such process with pid {pid} was running"));
        }
    }
    match backend {
        Backend::Gnome => {
            let (sender, receiver) = mpsc::sync_channel(0);
            if let Some(t) = time {
                thread::spawn(move || {
                    thread::sleep(t);
                    let _ = sender.send(());
                });
            }
            let _guard = gnome::inhibit_sleep()?;
            println!("Sleep blocked until the first process exits.");
            loop {
                if receiver.try_recv().is_ok() {
                    println!("Timeout reached before any process could exit.");
                    break;
                }
                for pid in pids {
                    if !pid.is_running() {
                        println!("Processes with pid {pid} exited.");
                        break;
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
        _ => unimplemented!(),
    }
    Ok(())
}

fn block_sleep_on_all_pids(
    pids: &[Pid],
    time: Option<Duration>,
    backend: Backend,
) -> anyhow::Result<()> {
    // Check that all the pids supplied are actually running. Only warn if one given is not
    // running as we're waiting for all of them to exit anyway.
    for pid in pids {
        if !pid.is_running() {
            println!(
                "{warn}: No such process with pid {pid} was running. Continuing.",
                warn = "warn:".yellow().bold()
            );
        }
    }
    match backend {
        Backend::Gnome => {
            let (sender, receiver) = mpsc::sync_channel(0);
            if let Some(t) = time {
                thread::spawn(move || {
                    thread::sleep(t);
                    let _ = sender.send(());
                });
            }
            let _guard = gnome::inhibit_sleep()?;
            println!("Sleep blocked until all processes exit.");
            loop {
                if receiver.try_recv().is_ok() {
                    println!("Timeout reached before all process could exit.");
                    break;
                }
                let any_running = pids
                    .iter()
                    .map(|pid| pid.is_running())
                    .any(|running| running);
                if !any_running {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
        _ => unimplemented!(),
    }
    Ok(())
}

fn block_sleep_indefinitely(backend: Backend) -> anyhow::Result<()> {
    match backend {
        Backend::Gnome => {
            let _guard = gnome::inhibit_sleep()?;
            println!("Sleep blocked indefinitely. Press CTRL-C to exit.");
            loop {
                thread::sleep(Duration::from_secs(u64::MAX));
            }
        }
        _ => unimplemented!("Backends other than Gnome are not implemented"),
    }
}
