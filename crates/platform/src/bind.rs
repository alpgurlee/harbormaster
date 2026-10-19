use std::io;
use std::net::{IpAddr, SocketAddr};

use socket2::{Domain, Socket, Type};

/// What happened when we tried to take a port for a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindProbe {
    Free,
    InUse,
    /// The OS refused, e.g. a privileged port or a range Windows reserved.
    Denied,
    /// This machine has no such address, e.g. no IPv6 loopback.
    Unavailable,
    Failed(String),
}

/// Binds and immediately releases `address:port`. Unlike `TcpListener::bind`,
/// the socket does not set SO_REUSEADDR, so a listener on the same port, even
/// on the wildcard address, makes the probe fail instead of slipping past it.
///
/// Windows lets a socket bind one address while another socket of the same
/// user holds the port on every address, and says only "denied" when a port
/// is held for one socket alone. So there the port is also asked for on
/// every address of the family, alone, which fails while any socket holds
/// it: a port held on another address counts as in use too.
pub fn probe_bind(address: IpAddr, port: u16) -> BindProbe {
    let here = attempt(address, port, &|_| Ok(()));
    #[cfg(windows)]
    if matches!(here, BindProbe::Free | BindProbe::Denied) {
        let anywhere = attempt(crate::sys::every(address), port, &crate::sys::alone);
        if anywhere != BindProbe::Free {
            return anywhere;
        }
    }
    here
}

fn attempt(address: IpAddr, port: u16, prepare: &dyn Fn(&Socket) -> io::Result<()>) -> BindProbe {
    let target = SocketAddr::new(address, port);
    let socket = match Socket::new(Domain::for_address(target), Type::STREAM, None) {
        Ok(socket) => socket,
        Err(error) => return failure(&error),
    };
    if address.is_ipv6() && socket.set_only_v6(true).is_err() {
        return BindProbe::Failed("could not restrict the probe to IPv6".into());
    }
    if let Err(error) = prepare(&socket) {
        return BindProbe::Failed(format!("could not set up the probe: {error}"));
    }
    match socket.bind(&target.into()) {
        Ok(()) => BindProbe::Free,
        Err(error) => failure(&error),
    }
}

#[cfg(target_os = "macos")]
const FAMILY_NOT_SUPPORTED: i32 = libc::EAFNOSUPPORT;
#[cfg(windows)]
const FAMILY_NOT_SUPPORTED: i32 = 10047; // WSAEAFNOSUPPORT

fn failure(error: &io::Error) -> BindProbe {
    match error.kind() {
        io::ErrorKind::AddrInUse => BindProbe::InUse,
        io::ErrorKind::PermissionDenied => BindProbe::Denied,
        io::ErrorKind::AddrNotAvailable => BindProbe::Unavailable,
        _ if error.raw_os_error() == Some(FAMILY_NOT_SUPPORTED) => BindProbe::Unavailable,
        _ => BindProbe::Failed(error.to_string()),
    }
}
