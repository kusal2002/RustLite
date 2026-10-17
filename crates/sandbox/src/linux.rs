//! Linux implementation: `no_new_privs`, Landlock and a seccomp-bpf filter,
//! applied by the renderer to itself. No user namespaces (they are often
//! disabled for unprivileged users, e.g. on Ubuntu 24.04).
//!
//! Landlock restricts the calling thread and the threads it creates later;
//! [`lockdown`] must run before the renderer starts threads. The seccomp
//! filter is synchronized to every thread (`SECCOMP_FILTER_FLAG_TSYNC`).

use std::io;

use crate::Lockdown;

/// `LANDLOCK_CREATE_RULESET_VERSION`: query the supported ABI version.
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

/// `struct landlock_ruleset_attr` (ABI 4 layout; ABI 1–3 read only the
/// first field, so they are given its size alone).
#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
}

/// Every filesystem access right the running kernel's Landlock ABI knows.
fn fs_rights(abi: i64) -> u64 {
    // ABI 1: EXECUTE … MAKE_SYM (bits 0–12).
    let mut rights = (1 << 13) - 1;
    if abi >= 2 {
        rights |= 1 << 13; // REFER
    }
    if abi >= 3 {
        rights |= 1 << 14; // TRUNCATE
    }
    if abi >= 5 {
        rights |= 1 << 15; // IOCTL_DEV
    }
    rights
}

fn check(r: libc::c_long) -> io::Result<libc::c_long> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

#[allow(unsafe_code)]
fn no_new_privs() -> io::Result<()> {
    // SAFETY: prctl with integer arguments only.
    check(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }.into()).map(drop)
}

/// A Landlock domain that handles every filesystem (and, from ABI 4, TCP
/// bind/connect) right and grants none. Returns the ABI version used.
#[allow(unsafe_code)]
fn landlock() -> io::Result<i64> {
    // SAFETY: a null attribute with size 0 and the VERSION flag is the
    // documented ABI query; no memory is accessed.
    let abi = check(unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    })?;
    let attr = RulesetAttr {
        handled_access_fs: fs_rights(abi),
        handled_access_net: if abi >= 4 { 0b11 } else { 0 },
    };
    let size = if abi >= 4 {
        std::mem::size_of::<RulesetAttr>()
    } else {
        std::mem::size_of::<u64>()
    };
    // SAFETY: `attr` is a valid, initialized ruleset attribute; `size`
    // covers only fields the kernel's ABI knows; no flags.
    let fd = check(unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &raw const attr,
            size,
            0u32,
        )
    })?;
    let fd = fd as libc::c_int;
    // SAFETY: `fd` is the ruleset descriptor just created; no flags.
    let restricted = check(unsafe { libc::syscall(libc::SYS_landlock_restrict_self, fd, 0u32) });
    // SAFETY: `fd` is owned here and closed once.
    unsafe {
        libc::close(fd);
    }
    restricted.map(|_| abi)
}

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

/// System calls a renderer never needs and an attacker would want: network
/// sockets, running programs, debugging other processes, and kernel
/// surface (modules, BPF, namespaces, io_uring, …). They fail with EPERM.
const DENIED: &[libc::c_long] = &[
    libc::SYS_socket,
    libc::SYS_execve,
    libc::SYS_execveat,
    libc::SYS_ptrace,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_mount,
    libc::SYS_umount2,
    libc::SYS_pivot_root,
    libc::SYS_chroot,
    libc::SYS_unshare,
    libc::SYS_setns,
    libc::SYS_bpf,
    libc::SYS_perf_event_open,
    libc::SYS_userfaultfd,
    libc::SYS_keyctl,
    libc::SYS_add_key,
    libc::SYS_request_key,
    libc::SYS_init_module,
    libc::SYS_finit_module,
    libc::SYS_delete_module,
    libc::SYS_kexec_load,
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_open_by_handle_at,
    libc::SYS_name_to_handle_at,
    libc::SYS_reboot,
    libc::SYS_swapon,
    libc::SYS_swapoff,
    libc::SYS_acct,
];

fn stmt(code: u32, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

/// The filter program: kill on a foreign architecture, EPERM for
/// [`DENIED`] (and x32 calls on x86_64), allow the rest.
fn program() -> Vec<libc::sock_filter> {
    use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_K, BPF_LD, BPF_RET, BPF_W};
    let deny = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;
    // Offsets into struct seccomp_data: nr (int) at 0, arch (u32) at 4.
    let mut p = vec![
        stmt(BPF_LD | BPF_W | BPF_ABS, 4),
        jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0),
        stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_KILL_PROCESS),
        stmt(BPF_LD | BPF_W | BPF_ABS, 0),
    ];
    #[cfg(target_arch = "x86_64")]
    {
        // x32 system calls (bit 30 set) reach the same kernel code under
        // other numbers: refuse them all.
        p.push(jump(BPF_JMP | libc::BPF_JGE | BPF_K, 0x4000_0000, 0, 1));
        p.push(stmt(BPF_RET | BPF_K, deny));
    }
    for &nr in DENIED {
        p.push(jump(BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, 1));
        p.push(stmt(BPF_RET | BPF_K, deny));
    }
    p.push(stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW));
    p
}

#[allow(unsafe_code)]
fn seccomp() -> io::Result<()> {
    let filter = program();
    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: `prog` points at `filter`, a valid BPF program of `len`
    // instructions that outlives the call (the kernel copies it);
    // no_new_privs is already set, as unprivileged filters require.
    check(unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_TSYNC,
            &raw const prog,
        )
    })
    .map(drop)
}

pub(crate) fn lockdown() -> Lockdown {
    let mut report = Lockdown::default();
    match no_new_privs() {
        Ok(()) => report.privileges = true,
        Err(e) => {
            // Both Landlock and unprivileged seccomp require it.
            tracing::warn!(error = %e, "could not set no_new_privs");
            return report;
        }
    }
    let landlock_abi = landlock()
        .inspect_err(|e| tracing::warn!(error = %e, "Landlock unavailable"))
        .ok();
    report.filesystem = landlock_abi.is_some();
    match seccomp() {
        Ok(()) => {
            report.network = true;
            report.processes = true;
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not install the seccomp filter");
            // Landlock ABI 4 still blocks TCP.
            report.network = landlock_abi.is_some_and(|abi| abi >= 4);
        }
    }
    tracing::debug!(?landlock_abi, "Linux sandbox applied");
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_program_fits_and_ends_in_allow() {
        let p = program();
        assert!(p.len() < 256, "jump offsets are u8");
        let last = p.last().unwrap();
        assert_eq!(last.k, libc::SECCOMP_RET_ALLOW);
        assert_eq!(fs_rights(1), 0x1FFF);
        assert_eq!(fs_rights(5), 0xFFFF);
    }
}
