use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::io::AsRawSocket;

use socket2::Socket;
use windows_sys::Win32::Networking::WinSock::{SO_EXCLUSIVEADDRUSE, SOL_SOCKET, setsockopt};

/// Every address of `address`'s family.
pub(crate) fn every(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    }
}

/// Asks that `socket` hold its port alone. Binding it then fails while any
/// other socket holds the port.
pub(crate) fn alone(socket: &Socket) -> io::Result<()> {
    let on: i32 = 1;
    // SAFETY: the socket is open, and the option takes an i32 of this size.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&raw const on).cast(),
            size_of::<i32>() as i32,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
