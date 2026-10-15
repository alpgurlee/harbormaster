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
    Failed(String),
}

/// Binds and immediately releases `address:port`. Unlike `TcpListener::bind`,
/// the socket does not set SO_REUSEADDR, so a listener on the same port, even
/// on the wildcard address, makes the probe fail instead of slipping past it.
pub fn probe_bind(address: IpAddr, port: u16) -> BindProbe {
    let target = SocketAddr::new(address, port);
    let socket = match Socket::new(Domain::for_address(target), Type::STREAM, None) {
        Ok(socket) => socket,
        Err(error) => return BindProbe::Failed(error.to_string()),
    };
    if address.is_ipv6() && socket.set_only_v6(true).is_err() {
        return BindProbe::Failed("could not restrict the probe to IPv6".into());
    }
    match socket.bind(&target.into()) {
        Ok(()) => BindProbe::Free,
        Err(error) => match error.kind() {
            io::ErrorKind::AddrInUse => BindProbe::InUse,
            io::ErrorKind::PermissionDenied => BindProbe::Denied,
            _ => BindProbe::Failed(error.to_string()),
        },
    }
}
