mod kinfo;
mod limits;
mod memory;
mod procargs;
mod process;
mod signal;
mod trash;

use std::collections::HashSet;
use std::ffi::{CStr, OsStr, c_int};
use std::io;
use std::mem::{MaybeUninit, size_of};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use libproc::file_info::pidfdinfo;
use libproc::net_info::{SocketFDInfo, SocketInfoKind, TcpSIState};

use crate::{Listener, SocketId, Whereabouts};

pub(crate) use limits::allow_more_open_files;
pub(crate) use memory::memory;
pub(crate) use process::{details, executable_of, processes, sets, tracks_exec};
pub(crate) use signal::{presence, signal};
pub(crate) use trash::trash;

// From xnu bsd/sys/proc_info.h.
const INI_IPV4: u8 = 0x1;
const INI_IPV6: u8 = 0x2;
const PROC_PIDFDVNODEPATHINFO: c_int = 2;

/// `struct proc_fileinfo` from xnu bsd/sys/proc_info.h.
#[repr(C)]
struct FileInfo {
    open_flags: u32,
    status: u32,
    offset: libc::off_t,
    kind: i32,
    guard_flags: u32,
}

/// `struct vnode_fdinfowithpath`: a file's details, then its vnode's, which
/// end with its path.
#[repr(C)]
struct VnodeWithPath {
    file: FileInfo,
    vnode: libc::vnode_info_path,
}

/// Each of this user's processes with its working folder, program and open files.
pub(crate) fn whereabouts() -> io::Result<Vec<Whereabouts>> {
    // SAFETY: getuid has no preconditions.
    let own = unsafe { libc::getuid() };
    let mut found = Vec::new();
    for pid in all_pids()? {
        let Ok((info, key)) = process::identify(pid) else {
            continue;
        };
        if info.pbi_uid != own {
            continue;
        }
        let mut paths = Vec::new();
        paths.extend(working_folder(pid));
        paths.extend(program_path(pid));
        if let Some(descriptors) = file_descriptors(pid) {
            paths.extend(
                descriptors
                    .iter()
                    .filter(|descriptor| descriptor.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
                    .filter_map(|descriptor| open_file(pid, descriptor.proc_fd)),
            );
        }
        // The pid may name another process by now, or another program.
        if !process::identify(pid).is_ok_and(|(after, again)| again == key && after.pbi_uid == own)
        {
            continue;
        }
        found.push(Whereabouts {
            key,
            name: process::name_of(&info),
            paths,
        });
    }
    Ok(found)
}

/// The path of the file `fd` names in `pid`, if it is still open.
fn open_file(pid: libc::pid_t, fd: i32) -> Option<PathBuf> {
    let size = c_int::try_from(size_of::<VnodeWithPath>()).ok()?;
    let mut info = MaybeUninit::<VnodeWithPath>::zeroed();
    // SAFETY: `info` is a zeroed buffer of exactly `size` bytes.
    let written = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDVNODEPATHINFO,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes; zeroes are valid for every field.
    path_of(&unsafe { info.assume_init() }.vnode)
}

/// The program `pid` runs, kept as bytes: a path need not be valid text.
fn program_path(pid: libc::pid_t) -> Option<PathBuf> {
    let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is writable for `size` bytes.
    let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), size) };
    if written <= 0 {
        return None;
    }
    let path = CStr::from_bytes_until_nul(&buffer).ok()?;
    (!path.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

/// Where `pid` is working.
fn working_folder(pid: libc::pid_t) -> Option<PathBuf> {
    let size = c_int::try_from(size_of::<libc::proc_vnodepathinfo>()).ok()?;
    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    // SAFETY: `info` is a zeroed buffer of exactly `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes; zeroes are valid for every field.
    path_of(&unsafe { info.assume_init() }.pvi_cdir)
}

/// The path at the end of a vnode's details, read no further than its buffer.
fn path_of(vnode: &libc::vnode_info_path) -> Option<PathBuf> {
    let buffer = &vnode.vip_path;
    // SAFETY: the nested arrays are one contiguous run of `size_of_val(buffer)` bytes.
    let bytes = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), std::mem::size_of_val(buffer))
    };
    let path = CStr::from_bytes_until_nul(bytes).ok()?;
    (!path.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

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
