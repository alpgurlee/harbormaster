use std::collections::HashMap;
use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use super::process::identify;
use crate::{Listener, ProcessKey};

/// How often to ask again for a table that grew while it was read.
const TRIES: usize = 8;

/// Each listening socket and the process that owns it. Windows reports no
/// socket identity, so `socket` is `None`.
pub(crate) fn listeners() -> io::Result<Vec<Listener>> {
    // Who each listening pid named before the table below was read, and
    // still names after: that process was there throughout, so the socket
    // the table gives the pid is its own.
    let before: HashMap<u32, ProcessKey> = rows()?
        .into_iter()
        .filter_map(|row| Some((row.pid, identify(row.pid)?)))
        .collect();
    let listeners = rows()?
        .into_iter()
        .filter_map(|row| {
            let owner = *before.get(&row.pid)?;
            (identify(row.pid)? == owner).then_some(Listener {
                owner,
                address: row.address,
                port: row.port,
                socket: None,
            })
        })
        .collect();
    Ok(listeners)
}

struct Row {
    pid: u32,
    address: IpAddr,
    port: u16,
}

fn rows() -> io::Result<Vec<Row>> {
    let v4 = table(AF_INET)?;
    let v6 = table(AF_INET6)?;
    // SAFETY: each table was filled for its family's rows.
    let (v4, v6) = unsafe {
        (
            entries::<MIB_TCPROW_OWNER_PID>(&v4),
            entries::<MIB_TCP6ROW_OWNER_PID>(&v6),
        )
    };
    let v4 = v4.iter().map(|row| Row {
        pid: row.dwOwningPid,
        address: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
        port: port(row.dwLocalPort),
    });
    let v6 = v6.iter().map(|row| Row {
        pid: row.dwOwningPid,
        address: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
        port: port(row.dwLocalPort),
    });
    Ok(v4.chain(v6).collect())
}

/// The listening sockets of one address family, as GetExtendedTcpTable lays
/// them out: a count, then the rows. Kept in u32s for the rows' alignment.
fn table(family: u16) -> io::Result<Vec<u32>> {
    let mut size = 0u32;
    for _ in 0..TRIES {
        let mut buffer = vec![0u32; (size as usize).div_ceil(4)];
        // SAFETY: the buffer holds at least `size` bytes, as many as the call may write.
        let result = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                u32::from(family),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match result {
            NO_ERROR => return Ok(buffer),
            // It grew since `size` was read; `size` now says how large it is.
            ERROR_INSUFFICIENT_BUFFER => continue,
            error => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
    Err(io::Error::other(
        "the table of listening sockets kept growing while it was read",
    ))
}

/// The rows of a table `table` read.
///
/// # Safety
///
/// `table` must hold a table GetExtendedTcpTable filled with rows of type `R`.
unsafe fn entries<R>(table: &[u32]) -> &[R] {
    let Some((&count, rows)) = table.split_first() else {
        return &[];
    };
    let room = size_of_val(rows) / size_of::<R>();
    let count = (count as usize).min(room);
    // SAFETY: the rows follow the count, aligned as their u32 fields need,
    // and `count` of them fit in what the table holds.
    unsafe { std::slice::from_raw_parts(rows.as_ptr().cast::<R>(), count) }
}

/// The tables keep a port in network byte order, in a u32's first two bytes.
fn port(raw: u32) -> u16 {
    let [high, low, ..] = raw.to_ne_bytes();
    u16::from_be_bytes([high, low])
}

#[cfg(test)]
mod tests {
    use super::{entries, port};
    use windows_sys::Win32::NetworkManagement::IpHelper::MIB_TCPROW_OWNER_PID;

    #[test]
    fn reads_a_port_kept_in_network_byte_order() {
        assert_eq!(port(u32::from_ne_bytes([0x0B, 0xB8, 0, 0])), 3000);
        assert_eq!(port(u32::from_ne_bytes([0x00, 0x50, 0x12, 0x34])), 80);
    }

    #[test]
    fn reads_no_more_rows_than_a_table_holds() {
        // A count of 5, but room for one row of six u32s.
        let table = [5, 0, 0, 0, 0, 0, 42];

        // SAFETY: the table is laid out as GetExtendedTcpTable's are.
        let rows = unsafe { entries::<MIB_TCPROW_OWNER_PID>(&table) };

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].dwOwningPid, 42);
    }
}
