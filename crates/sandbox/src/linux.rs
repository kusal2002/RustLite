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

/// The `clone` flags glibc's `pthread_create` passes: a thread in this
/// process, nothing else (no fork, no `vfork`, no namespaces).
const THREAD_FLAGS: u32 = (libc::CLONE_VM
    | libc::CLONE_FS
    | libc::CLONE_FILES
    | libc::CLONE_SIGHAND
    | libc::CLONE_THREAD
    | libc::CLONE_SYSVSEM
    | libc::CLONE_SETTLS
    | libc::CLONE_PARENT_SETTID
    | libc::CLONE_CHILD_CLEARTID) as u32;

/// What the filter does with one system call. Argument checks look at the
/// low 32 bits, which is all the kernel reads for every argument checked
/// here (`int`/`unsigned int` parameters, and `clone` flags, which it
/// truncates).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    /// Allowed with any arguments.
    Allow,
    /// Allowed when argument `arg` has none of the bits in `mask`.
    Without { arg: u32, mask: u32 },
    /// Allowed when argument `arg` is one of `values`.
    OneOf { arg: u32, values: Vec<u32> },
    /// Fails with this error number.
    Errno(i32),
}

/// The system calls a locked-down renderer makes: IPC over its stdio pipes
/// and logging to stderr, memory, threads and futexes, signals (Rust's
/// stack-overflow handler, `abort`), clocks, sleeping and randomness.
/// Everything else fails with `EPERM`. `pid` is the renderer's own
/// process id, the only one `tgkill` may signal.
fn rules(pid: u32) -> Vec<(libc::c_long, Rule)> {
    use Rule::{Allow, Errno, OneOf, Without};
    let no_exec = Without {
        arg: 2,
        mask: libc::PROT_EXEC as u32,
    };
    #[allow(unused_mut)]
    let mut rules = vec![
        // IPC and logs.
        (libc::SYS_read, Allow),
        (libc::SYS_readv, Allow),
        (libc::SYS_write, Allow),
        (libc::SYS_writev, Allow),
        (libc::SYS_close, Allow),
        (libc::SYS_fstat, Allow),
        (libc::SYS_ppoll, Allow),
        (
            libc::SYS_fcntl,
            OneOf {
                arg: 1,
                values: [libc::F_GETFD, libc::F_SETFD, libc::F_GETFL, libc::F_SETFL]
                    .map(|c| c as u32)
                    .to_vec(),
            },
        ),
        // `isatty` only.
        (
            libc::SYS_ioctl,
            OneOf {
                arg: 1,
                values: vec![libc::TCGETS as u32],
            },
        ),
        // Memory, never executable: the renderer loads no code and Boa has
        // no JIT.
        (libc::SYS_brk, Allow),
        (libc::SYS_mmap, no_exec.clone()),
        (libc::SYS_mprotect, no_exec),
        (libc::SYS_munmap, Allow),
        (libc::SYS_mremap, Allow),
        (libc::SYS_madvise, Allow),
        // Threads. glibc tries `clone3` first, whose flags a filter cannot
        // read; ENOSYS makes it fall back to `clone`.
        (
            libc::SYS_clone,
            OneOf {
                arg: 0,
                values: vec![THREAD_FLAGS],
            },
        ),
        (libc::SYS_clone3, Errno(libc::ENOSYS)),
        (libc::SYS_futex, Allow),
        (libc::SYS_set_robust_list, Allow),
        (libc::SYS_rseq, Allow),
        (libc::SYS_sched_yield, Allow),
        (libc::SYS_sched_getaffinity, Allow),
        (libc::SYS_gettid, Allow),
        (libc::SYS_getpid, Allow),
        (
            libc::SYS_prctl,
            OneOf {
                arg: 0,
                values: vec![libc::PR_SET_NAME as u32, libc::PR_GET_NAME as u32],
            },
        ),
        (libc::SYS_exit, Allow),
        (libc::SYS_exit_group, Allow),
        // Signals, delivered within this process only.
        (libc::SYS_rt_sigreturn, Allow),
        (libc::SYS_rt_sigprocmask, Allow),
        (libc::SYS_rt_sigaction, Allow),
        (libc::SYS_sigaltstack, Allow),
        (libc::SYS_restart_syscall, Allow),
        (
            libc::SYS_tgkill,
            OneOf {
                arg: 0,
                values: vec![pid],
            },
        ),
        // Time (usually the vDSO) and randomness.
        (libc::SYS_clock_gettime, Allow),
        (libc::SYS_clock_getres, Allow),
        (libc::SYS_gettimeofday, Allow),
        (libc::SYS_nanosleep, Allow),
        (libc::SYS_clock_nanosleep, Allow),
        (libc::SYS_getrandom, Allow),
    ];
    #[cfg(target_arch = "x86_64")]
    rules.extend([(libc::SYS_poll, Allow), (libc::SYS_time, Allow)]);
    rules
}

fn stmt(code: u32, k: u32) -> libc::sock_filter {
    jump(code, k, 0, 0)
}

fn jump(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

/// Offset of the low 32 bits of argument `arg` in `struct seccomp_data`
/// (`nr`, `arch`, `instruction_pointer`, then six `u64` arguments; both
/// architectures are little-endian).
fn arg_offset(arg: u32) -> u32 {
    16 + 8 * arg
}

/// The instructions that decide one listed system call. Every path ends in
/// a return, so the program never jumps further than one rule's body and
/// the u8 jump offsets cannot overflow however long the list grows.
fn body(rule: &Rule) -> Vec<libc::sock_filter> {
    use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W};
    let allow = stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ALLOW);
    let deny = stmt(BPF_RET | BPF_K, DENY);
    match rule {
        Rule::Allow => vec![allow],
        Rule::Errno(e) => vec![stmt(BPF_RET | BPF_K, libc::SECCOMP_RET_ERRNO | *e as u32)],
        Rule::Without { arg, mask } => vec![
            stmt(BPF_LD | BPF_W | BPF_ABS, arg_offset(*arg)),
            jump(BPF_JMP | BPF_JSET | BPF_K, *mask, 1, 0),
            allow,
            deny,
        ],
        Rule::OneOf { arg, values } => {
            let n = values.len();
            let mut b = vec![stmt(BPF_LD | BPF_W | BPF_ABS, arg_offset(*arg))];
            for (i, &v) in values.iter().enumerate() {
                // Match: skip the remaining checks and the deny.
                let to_allow = u8::try_from(n - i).expect("few values per rule");
                b.push(jump(BPF_JMP | BPF_JEQ | BPF_K, v, to_allow, 0));
            }
            b.push(deny);
            b.push(allow);
            b
        }
    }
}

/// What every system call not in [`rules`] gets.
const DENY: u32 = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;

/// The filter program: kill on a foreign architecture, EPERM for x32 calls
/// on x86_64, then [`rules`], then EPERM for everything else.
fn program(pid: u32) -> Vec<libc::sock_filter> {
    use libc::{BPF_ABS, BPF_JEQ, BPF_JMP, BPF_K, BPF_LD, BPF_RET, BPF_W};
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
        p.push(stmt(BPF_RET | BPF_K, DENY));
    }
    for (nr, rule) in rules(pid) {
        let body = body(&rule);
        // Not this call: skip its body; the accumulator still holds `nr`
        // (a body that runs always returns).
        let skip = u8::try_from(body.len()).expect("short rule bodies");
        p.push(jump(BPF_JMP | BPF_JEQ | BPF_K, nr as u32, 0, skip));
        p.extend(body);
    }
    p.push(stmt(BPF_RET | BPF_K, DENY));
    p
}

#[allow(unsafe_code)]
fn seccomp() -> io::Result<()> {
    let filter = program(std::process::id());
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

/// Calls `getppid`, which [`rules`] does not list, and reports whether it
/// failed with `EPERM` (it always succeeds outside the filter).
#[allow(unsafe_code)]
pub(crate) fn unlisted_syscall_denied() -> bool {
    // SAFETY: getppid takes no arguments and touches no memory.
    let r = unsafe { libc::syscall(libc::SYS_getppid) };
    r == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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

    const PID: u32 = 4242;

    /// Runs the filter the way the kernel does, for the instructions it
    /// uses.
    fn run(nr: libc::c_long, arch: u32, args: [u64; 6]) -> u32 {
        use libc::{BPF_ABS, BPF_JEQ, BPF_JGE, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W};
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(&(nr as u32).to_le_bytes());
        data[4..8].copy_from_slice(&arch.to_le_bytes());
        for (i, a) in args.iter().enumerate() {
            data[16 + 8 * i..24 + 8 * i].copy_from_slice(&a.to_le_bytes());
        }
        let p = program(PID);
        let (mut pc, mut acc) = (0usize, 0u32);
        loop {
            let i = p[pc];
            let code = u32::from(i.code);
            pc += 1;
            if code == BPF_LD | BPF_W | BPF_ABS {
                let k = i.k as usize;
                acc = u32::from_le_bytes(data[k..k + 4].try_into().unwrap());
            } else if code == BPF_RET | BPF_K {
                return i.k;
            } else {
                let taken = match code {
                    c if c == BPF_JMP | BPF_JEQ | BPF_K => acc == i.k,
                    c if c == BPF_JMP | BPF_JGE | BPF_K => acc >= i.k,
                    c if c == BPF_JMP | BPF_JSET | BPF_K => acc & i.k != 0,
                    c => panic!("unexpected instruction {c:#x}"),
                };
                pc += usize::from(if taken { i.jt } else { i.jf });
            }
        }
    }

    fn call(nr: libc::c_long, args: [u64; 6]) -> u32 {
        run(nr, AUDIT_ARCH, args)
    }

    const ALLOW: u32 = libc::SECCOMP_RET_ALLOW;

    #[test]
    fn listed_calls_pass_and_everything_else_is_eperm() {
        assert_eq!(call(libc::SYS_write, [2, 0, 0, 0, 0, 0]), ALLOW);
        assert_eq!(call(libc::SYS_futex, [0; 6]), ALLOW);
        assert_eq!(call(libc::SYS_getrandom, [0; 6]), ALLOW);
        for nr in [
            libc::SYS_getppid,
            libc::SYS_socket,
            libc::SYS_execve,
            libc::SYS_openat,
            libc::SYS_newfstatat,
            libc::SYS_ptrace,
            libc::SYS_kill,
            libc::SYS_bpf,
            libc::SYS_io_uring_setup,
            libc::SYS_seccomp,
        ] {
            assert_eq!(call(nr, [0; 6]), DENY, "syscall {nr}");
        }
        assert!(rules(PID).iter().all(|(nr, _)| *nr != libc::SYS_getppid));
    }

    #[test]
    fn foreign_architectures_are_killed() {
        assert_eq!(
            run(libc::SYS_write, 0x4000_0003, [0; 6]),
            libc::SECCOMP_RET_KILL_PROCESS
        );
        #[cfg(target_arch = "x86_64")]
        assert_eq!(call(0x4000_0000 | libc::SYS_write, [0; 6]), DENY);
    }

    #[test]
    fn arguments_are_checked() {
        let rw = (libc::PROT_READ | libc::PROT_WRITE) as u64;
        let rx = (libc::PROT_READ | libc::PROT_EXEC) as u64;
        assert_eq!(call(libc::SYS_mmap, [0, 4096, rw, 0, 0, 0]), ALLOW);
        assert_eq!(call(libc::SYS_mmap, [0, 4096, rx, 0, 0, 0]), DENY);
        assert_eq!(call(libc::SYS_mprotect, [0, 4096, rx, 0, 0, 0]), DENY);

        let thread = u64::from(THREAD_FLAGS);
        assert_eq!(call(libc::SYS_clone, [thread, 0, 0, 0, 0, 0]), ALLOW);
        let fork = libc::SIGCHLD as u64;
        assert_eq!(call(libc::SYS_clone, [fork, 0, 0, 0, 0, 0]), DENY);
        let vfork = (libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD) as u64;
        assert_eq!(call(libc::SYS_clone, [vfork, 0, 0, 0, 0, 0]), DENY);
        let ns = thread | libc::CLONE_NEWUSER as u64;
        assert_eq!(call(libc::SYS_clone, [ns, 0, 0, 0, 0, 0]), DENY);
        assert_eq!(
            call(libc::SYS_clone3, [0; 6]),
            libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32
        );

        assert_eq!(call(libc::SYS_tgkill, [PID.into(), 1, 6, 0, 0, 0]), ALLOW);
        assert_eq!(call(libc::SYS_tgkill, [1, 1, 9, 0, 0, 0]), DENY);
        assert_eq!(call(libc::SYS_ioctl, [0, libc::TCGETS, 0, 0, 0, 0]), ALLOW);
        assert_eq!(call(libc::SYS_ioctl, [0, libc::TIOCSTI, 0, 0, 0, 0]), DENY);
        let getfl = libc::F_GETFL as u64;
        assert_eq!(call(libc::SYS_fcntl, [0, getfl, 0, 0, 0, 0]), ALLOW);
        let dup = libc::F_DUPFD as u64;
        assert_eq!(call(libc::SYS_fcntl, [0, dup, 0, 0, 0, 0]), DENY);
        let set_name = libc::PR_SET_NAME as u64;
        assert_eq!(call(libc::SYS_prctl, [set_name, 0, 0, 0, 0, 0]), ALLOW);
        let dumpable = libc::PR_SET_DUMPABLE as u64;
        assert_eq!(call(libc::SYS_prctl, [dumpable, 1, 0, 0, 0, 0]), DENY);
    }

    #[test]
    fn program_fits_the_kernel_limit() {
        let p = program(PID);
        // BPF_MAXINSNS.
        assert!(p.len() <= 4096, "{} instructions", p.len());
        assert_eq!(p.last().unwrap().k, DENY);
        assert_eq!(fs_rights(1), 0x1FFF);
        assert_eq!(fs_rights(5), 0xFFFF);
    }
}
