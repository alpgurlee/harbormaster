mod kinfo;
mod memory;
mod procargs;
mod process;
mod signal;

use std::collections::HashSet;
use std::ffi::c_int;
use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use libproc::file_info::pidfdinfo;
use libproc::net_info::{SocketFDInfo, SocketInfoKind, TcpSIState};

use crate::{Listener, SocketId};

pub(crate) use memory::memory;
pub(crate) use process::{details, executable_of, processes, tracks_exec};
pub(crate) use signal::{presence, signal};

// From xnu bsd/sys/proc_info.h.
const INI_IPV4: u8 = 0x1;
const INI_IPV6: u8 = 0x2;

pub(crate) fn listeners() -> io::Result<Vec<Listener>> {
    let mut seen = HashSet::new();
    let mut listeners = Vec::new();
    for pid in all_pids()? {
        let Ok((_, owner)) = process::identify(pid) else {
            continue;
        };
        let Some(descriptors) = file_descriptors(pid) else {
            continue;
        };
        let sockets: Vec<(IpAddr, u16, SocketId)> = descriptors
            .iter()
            .filter(|descriptor| descriptor.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            .filter_map(|descriptor| tcp_listen_address(pid, descriptor.proc_fd))
            .collect();
        if sockets.is_empty() {
            continue;
        }
        // Skip this round if the pid changed hands while we read its sockets.
        if !process::identify(pid).is_ok_and(|(_, after)| after == owner) {
            continue;
        }
        // A process can hold one socket under several descriptors.
        for (address, port, socket) in sockets {
            if seen.insert((owner, socket)) {
                listeners.push(Listener {
                    owner,
                    address,
                    port,
                    socket: Some(socket),
                });
            }
        }
    }
    Ok(listeners)
}

fn all_pids() -> io::Result<Vec<libc::pid_t>> {
    // SAFETY: a null buffer asks only for the current number of processes.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    // Leave room for processes started between the two calls.
    let capacity = count as usize + 64;
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    let bytes = c_int::try_from(capacity * size_of::<libc::pid_t>()).unwrap_or(c_int::MAX);
    // SAFETY: `pids` holds `capacity` initialised pid_t values and `bytes` is its size.
    let written = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    pids.truncate(written as usize);
    pids.retain(|&pid| pid > 0);
    Ok(pids)
}

/// Fails for processes we may not inspect, such as those of other users.
fn file_descriptors(pid: libc::pid_t) -> Option<Vec<libc::proc_fdinfo>> {
    // SAFETY: with a null buffer the kernel only reports the size it needs.
    let needed =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return None;
    }
    let capacity = needed as usize / size_of::<libc::proc_fdinfo>() + 16;
    let empty = libc::proc_fdinfo {
        proc_fd: 0,
        proc_fdtype: 0,
    };
    let mut descriptors = vec![empty; capacity];
    let bytes = c_int::try_from(capacity * size_of::<libc::proc_fdinfo>()).ok()?;
    // SAFETY: `descriptors` is an initialised buffer of exactly `bytes` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            bytes,
        )
    };
    if written <= 0 {
        return None;
    }
    descriptors.truncate(written as usize / size_of::<libc::proc_fdinfo>());
    Some(descriptors)
}

fn tcp_listen_address(pid: libc::pid_t, fd: i32) -> Option<(IpAddr, u16, SocketId)> {
    let info: SocketFDInfo = pidfdinfo(pid, fd).ok()?;
    if !matches!(SocketInfoKind::from(info.psi.soi_kind), SocketInfoKind::Tcp) {
        return None;
    }
    // SAFETY: the kernel fills `pri_tcp` whenever `soi_kind` is SOCKINFO_TCP.
    let tcp = unsafe { info.psi.soi_proto.pri_tcp };
    if tcp.tcpsi_state != TcpSIState::Listen as c_int {
        return None;
    }
    let inet = tcp.tcpsi_ini;
    let address = if inet.insi_vflag & INI_IPV6 != 0 {
        // SAFETY: INI_IPV6 marks `ina_6` as the active member.
        IpAddr::V6(Ipv6Addr::from(unsafe { inet.insi_laddr.ina_6.s6_addr }))
    } else if inet.insi_vflag & INI_IPV4 != 0 {
        // SAFETY: INI_IPV4 marks `ina_46` as the active member.
        let raw = unsafe { inet.insi_laddr.ina_46.i46a_addr4.s_addr };
        IpAddr::V4(Ipv4Addr::from(u32::from_be(raw)))
    } else {
        return None;
    };
    // The kernel keeps the port in network byte order in the low 16 bits.
    // `soi_so` is the kernel's socket address, the same for every process
    // holding this socket; the generation count tells apart sockets that
    // reused that address.
    Some((
        address,
        u16::from_be(inet.insi_lport as u16),
        SocketId {
            kernel: info.psi.soi_so,
            generation: inet.insi_gencnt,
        },
    ))
}
