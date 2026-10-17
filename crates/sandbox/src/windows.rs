//! Win32 implementation. Every `unsafe` block calls one documented Win32
//! function; handles are owned by RAII wrappers and closed exactly once.
//!
//! The launch follows the model of Chromium's Windows sandbox: the renderer
//! is created suspended with a *lockdown* primary token that can open
//! nothing, and its first thread is given a same-access *warm-up*
//! impersonation token so the loader can map the executable and DLLs. The
//! renderer drops the warm-up token in [`lockdown`] (`RevertToSelf`).

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_NO_TOKEN, GetLastError, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, DuplicateTokenEx,
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsTokenRestricted,
    RevertToSelf, SECURITY_ATTRIBUTES, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES,
    SecurityImpersonation, SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_IMPERSONATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenGroups, TokenImpersonation, TokenIntegrityLevel, TokenUser, WinLowLabelSid,
    WinNullSid,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessAsUserW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetCurrentThread, INFINITE,
    InitializeProcThreadAttributeList, OpenProcessToken, OpenThreadToken,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, ProcessDynamicCodePolicy,
    ProcessImageLoadPolicy, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    SetProcessMitigationPolicy, SetThreadToken, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

use crate::{Launched, Lockdown};

/// `SE_GROUP_INTEGRITY` (winnt.h): marks the SID as a mandatory label.
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
/// `SE_GROUP_ENABLED` (winnt.h).
const SE_GROUP_ENABLED: u32 = 0x0000_0004;

/// An owned kernel handle, closed on drop.
#[derive(Debug)]
struct Handle(HANDLE);

impl Handle {
    /// Hands the handle to a std type that will close it.
    fn into_owned(self) -> OwnedHandle {
        let raw = self.0;
        std::mem::forget(self);
        // SAFETY: `raw` is a valid handle this wrapper exclusively owned;
        // ownership moves to the OwnedHandle (we forgot `self`, so it is not
        // closed twice).
        #[allow(unsafe_code)]
        unsafe {
            OwnedHandle::from_raw_handle(raw)
        }
    }
}

impl Drop for Handle {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid handle this wrapper exclusively owns
        // (constructed only from successful Create/Open/Duplicate calls) and
        // it is closed exactly once, here.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// ----- job object ---------------------------------------------------------------------

#[derive(Debug)]
struct Job {
    handle: Handle,
}

impl Job {
    #[allow(unsafe_code)]
    fn new(memory_limit: usize) -> io::Result<Self> {
        // SAFETY: null security attributes and a null name are documented
        // as valid (default security, anonymous job).
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = Handle(raw);

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        // One job per renderer: the renderer itself is the only process
        // allowed, so any CreateProcess from it fails.
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = memory_limit;
        // SAFETY: `handle` is a valid job handle; the pointer and size
        // describe a fully initialized JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        // that outlives the call.
        check(unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        })?;

        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
        };
        // SAFETY: as above, for a JOBOBJECT_BASIC_UI_RESTRICTIONS value.
        check(unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectBasicUIRestrictions,
                (&raw const ui).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            )
        })?;
        Ok(Self { handle })
    }

    #[allow(unsafe_code)]
    fn assign(&self, process: &Handle) -> io::Result<()> {
        // SAFETY: both handles are valid for the duration of the call (owned
        // by `self` and borrowed `process`).
        check(unsafe { AssignProcessToJobObject(self.handle.0, process.0) })
    }
}

// ----- tokens ---------------------------------------------------------------------------

#[allow(unsafe_code)]
fn process_token(access: u32) -> io::Result<Handle> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
    // closing; `token` is a valid out-pointer.
    check(unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) })?;
    Ok(Handle(token))
}

/// Reads a variable-size token information class into an 8-byte aligned
/// buffer.
#[allow(unsafe_code)]
fn token_info(token: &Handle, class: i32) -> io::Result<Vec<u64>> {
    let mut len = 0u32;
    // SAFETY: a null buffer of length 0 is the documented way to query the
    // required size (the call fails with ERROR_INSUFFICIENT_BUFFER).
    unsafe {
        GetTokenInformation(token.0, class, std::ptr::null_mut(), 0, &mut len);
    }
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` is writable for at least `len` bytes and 8-byte aligned
    // (enough for every TOKEN_* structure).
    check(unsafe { GetTokenInformation(token.0, class, buf.as_mut_ptr().cast(), len, &mut len) })?;
    Ok(buf)
}

/// A well-known SID in a buffer of the maximum SID size.
#[allow(unsafe_code)]
fn well_known_sid(kind: i32) -> io::Result<Vec<u8>> {
    let mut sid = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut size = SECURITY_MAX_SID_SIZE;
    // SAFETY: `sid` is a writable buffer of `size` bytes (the documented
    // maximum SID size); no domain SID is needed for these SIDs.
    check(unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &mut size,
        )
    })?;
    sid.truncate(size as usize);
    Ok(sid)
}

#[allow(unsafe_code)]
fn set_low_integrity(token: &Handle) -> io::Result<()> {
    let mut sid = well_known_sid(WinLowLabelSid)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    // SAFETY: `token` was opened or created with TOKEN_ADJUST_DEFAULT;
    // `label` points at `sid`, which outlives the call; the length covers
    // the label struct plus the SID it references, as the API requires.
    check(unsafe {
        SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            (std::mem::size_of::<TOKEN_MANDATORY_LABEL>() + sid.len()) as u32,
        )
    })
}

/// Which renderer token to derive from the browser's.
#[derive(Clone, Copy, PartialEq)]
enum TokenKind {
    /// Primary token: user and every group deny-only, NULL SID as the only
    /// restricting SID, so access checks grant nothing.
    Lockdown,
    /// Impersonation token for start-up: restricted to the token's own SIDs
    /// (same access as the user), so a restricted process may impersonate it.
    WarmUp,
}

#[allow(unsafe_code)]
fn renderer_token(kind: TokenKind) -> io::Result<Handle> {
    let base =
        process_token(TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT)?;
    let user_buf = token_info(&base, TokenUser)?;
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID
    // pointer points into the same buffer, which outlives its uses below.
    let user = unsafe { &*user_buf.as_ptr().cast::<TOKEN_USER>() }.User;
    let groups_buf = token_info(&base, TokenGroups)?;
    // SAFETY: on success the buffer starts with a TOKEN_GROUPS header
    // followed by `GroupCount` SID_AND_ATTRIBUTES entries (the struct
    // declares one; the rest follow contiguously), all inside the buffer.
    let groups: &[SID_AND_ATTRIBUTES] = unsafe {
        let header = &*groups_buf.as_ptr().cast::<TOKEN_GROUPS>();
        std::slice::from_raw_parts(header.Groups.as_ptr(), header.GroupCount as usize)
    };
    let ordinary = groups
        .iter()
        .filter(|g| g.Attributes & SE_GROUP_INTEGRITY == 0);

    let mut null_sid = well_known_sid(WinNullSid)?;
    let (disable, restrict): (Vec<SID_AND_ATTRIBUTES>, Vec<SID_AND_ATTRIBUTES>) = match kind {
        TokenKind::Lockdown => {
            let mut disable: Vec<SID_AND_ATTRIBUTES> = ordinary
                .map(|g| SID_AND_ATTRIBUTES {
                    Sid: g.Sid,
                    Attributes: 0,
                })
                .collect();
            disable.push(SID_AND_ATTRIBUTES {
                Sid: user.Sid,
                Attributes: 0,
            });
            let restrict = vec![SID_AND_ATTRIBUTES {
                Sid: null_sid.as_mut_ptr().cast(),
                Attributes: 0,
            }];
            (disable, restrict)
        }
        TokenKind::WarmUp => {
            let mut restrict: Vec<SID_AND_ATTRIBUTES> = ordinary
                .filter(|g| g.Attributes & SE_GROUP_ENABLED != 0)
                .map(|g| SID_AND_ATTRIBUTES {
                    Sid: g.Sid,
                    Attributes: 0,
                })
                .collect();
            restrict.push(SID_AND_ATTRIBUTES {
                Sid: user.Sid,
                Attributes: 0,
            });
            (Vec::new(), restrict)
        }
    };

    let mut restricted: HANDLE = std::ptr::null_mut();
    // SAFETY: `base` is a valid token opened with TOKEN_DUPLICATE |
    // TOKEN_QUERY; the SID arrays (and the SIDs they point to, in
    // `user_buf`, `groups_buf` and `null_sid`) outlive the call and their
    // lengths are passed with them; no privileges are listed (null, 0).
    check(unsafe {
        CreateRestrictedToken(
            base.0,
            DISABLE_MAX_PRIVILEGE,
            disable.len() as u32,
            disable.as_ptr(),
            0,
            std::ptr::null(),
            restrict.len() as u32,
            restrict.as_ptr(),
            &mut restricted,
        )
    })?;
    let restricted = Handle(restricted);
    set_low_integrity(&restricted)?;
    if kind == TokenKind::Lockdown {
        return Ok(restricted);
    }
    let mut impersonation: HANDLE = std::ptr::null_mut();
    // SAFETY: `restricted` is a valid token created above with all access;
    // null attributes mean default security; the out-pointer is valid.
    check(unsafe {
        DuplicateTokenEx(
            restricted.0,
            TOKEN_IMPERSONATE | TOKEN_QUERY,
            std::ptr::null(),
            SecurityImpersonation,
            TokenImpersonation,
            &mut impersonation,
        )
    })?;
    Ok(Handle(impersonation))
}

// ----- launching ------------------------------------------------------------------------

/// A renderer process (its handle) and the job it runs in.
#[derive(Debug)]
pub(crate) struct Process {
    handle: Handle,
    pid: u32,
    _job: Job,
}

impl Process {
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    #[allow(unsafe_code)]
    pub(crate) fn kill(&mut self) -> io::Result<()> {
        // SAFETY: `handle` is the live process handle owned by `self`.
        check(unsafe { TerminateProcess(self.handle.0, 1) })
    }

    #[allow(unsafe_code)]
    pub(crate) fn wait(&mut self) -> io::Result<()> {
        // SAFETY: as above; INFINITE is a valid timeout.
        let r = unsafe { WaitForSingleObject(self.handle.0, INFINITE) };
        if r == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// An anonymous pipe; the child's end is inheritable, the browser's is not.
#[allow(unsafe_code)]
fn pipe(child_reads: bool) -> io::Result<(Handle, Handle)> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: valid out-pointers and a fully initialized SECURITY_ATTRIBUTES;
    // 0 selects the default buffer size.
    check(unsafe { CreatePipe(&mut read, &mut write, &sa, 0) })?;
    let (read, write) = (Handle(read), Handle(write));
    let (child, parent) = if child_reads {
        (read, write)
    } else {
        (write, read)
    };
    // SAFETY: `parent` is a valid handle owned above.
    check(unsafe { SetHandleInformation(parent.0, HANDLE_FLAG_INHERIT, 0) })?;
    Ok((child, parent))
}

/// An inheritable duplicate of the browser's stderr, if it has one.
#[allow(unsafe_code)]
fn inheritable_stderr() -> Option<Handle> {
    // SAFETY: no arguments besides a valid STD_HANDLE constant.
    let err = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if err.is_null() || err == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: duplicating a handle of the current process into the current
    // process (pseudo-handles), with a valid out-pointer.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            err,
            GetCurrentProcess(),
            &mut dup,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (ok != 0).then(|| Handle(dup))
}

/// Quotes one argument for the MSVC command-line parser.
fn quote(arg: &OsStr, out: &mut Vec<u16>) {
    let wide: Vec<u16> = arg.encode_wide().collect();
    let plain = !wide.is_empty()
        && !wide
            .iter()
            .any(|&c| c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'"'));
    if plain {
        out.extend_from_slice(&wide);
        return;
    }
    out.push(u16::from(b'"'));
    let mut backslashes = 0;
    for &c in &wide {
        if c == u16::from(b'\\') {
            backslashes += 1;
        } else {
            if c == u16::from(b'"') {
                // Escape the backslashes before a quote, then the quote.
                out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(c);
    }
    // Backslashes before the closing quote must be doubled.
    out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
    out.push(u16::from(b'"'));
}

/// A PROC_THREAD_ATTRIBUTE_LIST that limits inheritance to `handles`.
struct AttributeList {
    buf: Vec<u64>,
    /// Must stay at a fixed address while the list refers to it.
    _handles: Box<[HANDLE]>,
}

impl AttributeList {
    #[allow(unsafe_code)]
    fn handle_list(handles: Box<[HANDLE]>) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: a null list with a size out-pointer queries the size.
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
        }
        let mut list = Self {
            buf: vec![0u64; size.div_ceil(8)],
            _handles: handles,
        };
        // SAFETY: `buf` is writable for `size` bytes and 8-byte aligned.
        check(unsafe {
            InitializeProcThreadAttributeList(list.buf.as_mut_ptr().cast(), 1, 0, &mut size)
        })?;
        // SAFETY: the list was initialized for one attribute; the value
        // points at the boxed handle array, which lives as long as `list`
        // (the list stores the pointer, not a copy).
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list.buf.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                list._handles.as_ptr().cast(),
                std::mem::size_of_val(&*list._handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            // SAFETY: the list was initialized above.
            unsafe { DeleteProcThreadAttributeList(list.buf.as_mut_ptr().cast()) };
            return Err(e);
        }
        Ok(list)
    }
}

impl Drop for AttributeList {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the list was initialized in `handle_list` and is deleted
        // exactly once.
        unsafe { DeleteProcThreadAttributeList(self.buf.as_mut_ptr().cast()) };
    }
}

#[allow(unsafe_code)]
pub(crate) fn launch(exe: &Path, args: &[&OsStr], memory_limit: usize) -> io::Result<Launched> {
    let job = Job::new(memory_limit)?;
    let lockdown_token = renderer_token(TokenKind::Lockdown)?;
    let warm_up = renderer_token(TokenKind::WarmUp)?;
    let (child_in, parent_in) = pipe(true)?;
    let (child_out, parent_out) = pipe(false)?;
    let child_err = inheritable_stderr();

    let mut inherit = vec![child_in.0, child_out.0];
    inherit.extend(child_err.as_ref().map(|h| h.0));
    let attributes = AttributeList::handle_list(inherit.into_boxed_slice())?;

    let mut cmdline = Vec::new();
    quote(exe.as_os_str(), &mut cmdline);
    for a in args {
        cmdline.push(u16::from(b' '));
        quote(a, &mut cmdline);
    }
    cmdline.push(0);
    let app: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();

    let mut si = STARTUPINFOEXW::default();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = child_in.0;
    si.StartupInfo.hStdOutput = child_out.0;
    si.StartupInfo.hStdError = child_err.as_ref().map_or(std::ptr::null_mut(), |h| h.0);
    si.lpAttributeList = attributes.buf.as_ptr().cast_mut().cast();
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: the token is a primary restricted version of this process's
    // token (so no special privilege is needed); `app` and `cmdline` are
    // NUL-terminated wide strings (`cmdline` writable, as required); null
    // attributes/environment/directory mean defaults; `si` is a complete
    // STARTUPINFOEXW whose attribute list and handles outlive the call; `pi`
    // is a valid out-pointer.
    check(unsafe {
        CreateProcessAsUserW(
            lockdown_token.0,
            app.as_ptr(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null(),
            std::ptr::null(),
            &si.StartupInfo,
            &mut pi,
        )
    })?;
    let process = Handle(pi.hProcess);
    let thread = Handle(pi.hThread);
    let mut process = Process {
        handle: process,
        pid: pi.dwProcessId,
        _job: job,
    };
    let started = (|| {
        process._job.assign(&process.handle)?;
        // SAFETY: `thread` is the suspended primary thread (full access) and
        // `warm_up` a valid impersonation token; the thread pointer is valid
        // for the call.
        check(unsafe { SetThreadToken(&thread.0, warm_up.0) })?;
        // SAFETY: `thread` is a valid, suspended thread handle.
        if unsafe { ResumeThread(thread.0) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if let Err(e) = started {
        let _ = process.kill();
        return Err(e);
    }
    Ok(Launched {
        process: crate::Process { inner: process },
        stdin: Box::new(std::fs::File::from(parent_in.into_owned())),
        stdout: Box::new(std::fs::File::from(parent_out.into_owned())),
    })
}

// ----- inside the renderer --------------------------------------------------------------

/// Sets a mitigation policy whose structure is a single `u32` of flags
/// (PROCESS_MITIGATION_*_POLICY are 4-byte unions of bitfields).
#[allow(unsafe_code)]
fn mitigation(policy: i32, flags: u32) -> io::Result<()> {
    // SAFETY: the buffer is a u32 the size of the policy structure, valid
    // for the duration of the call.
    check(unsafe {
        SetProcessMitigationPolicy(
            policy,
            (&raw const flags).cast(),
            std::mem::size_of::<u32>(),
        )
    })
}

#[allow(unsafe_code)]
fn primary_token_restricted() -> io::Result<bool> {
    let token = process_token(TOKEN_QUERY)?;
    // SAFETY: `token` is a valid token handle with TOKEN_QUERY access.
    Ok(unsafe { IsTokenRestricted(token.0) } != 0)
}

/// Whether the current thread is impersonating (`Ok(false)`: it runs with
/// the process's primary token).
#[allow(unsafe_code)]
fn thread_impersonating() -> io::Result<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: pseudo-handle for the current thread; OpenAsSelf = TRUE checks
    // access against the process token; valid out-pointer.
    let ok = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
    if ok != 0 {
        drop(Handle(token));
        return Ok(true);
    }
    // SAFETY: reads this thread's last-error value.
    match unsafe { GetLastError() } {
        ERROR_NO_TOKEN => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

pub(crate) fn lockdown() -> Lockdown {
    let mut report = Lockdown {
        integrity: integrity_level(),
        ..Lockdown::default()
    };
    // Queried while the warm-up token still allows opening our own token.
    let restricted = primary_token_restricted().unwrap_or(false);
    let in_job = in_job().unwrap_or(false);
    // PROCESS_MITIGATION_DYNAMIC_CODE_POLICY: ProhibitDynamicCode (bit 0).
    let dynamic = mitigation(ProcessDynamicCodePolicy, 1)
        .inspect_err(|e| tracing::warn!(error = %e, "could not prohibit dynamic code"))
        .is_ok();
    // PROCESS_MITIGATION_IMAGE_LOAD_POLICY: NoRemoteImages (bit 0),
    // NoLowMandatoryLabelImages (bit 1), PreferSystem32Images (bit 2).
    let images = mitigation(ProcessImageLoadPolicy, 0b111)
        .inspect_err(|e| tracing::warn!(error = %e, "could not restrict image loads"))
        .is_ok();
    // Drop the warm-up token: from here on the restricted primary token
    // decides every access check.
    #[allow(unsafe_code)]
    // SAFETY: no arguments; reverts the calling thread's impersonation.
    let reverted = unsafe { RevertToSelf() } != 0;
    let impersonating = thread_impersonating().unwrap_or(true);
    let locked = restricted && reverted && !impersonating;
    if !locked {
        tracing::warn!(
            restricted,
            reverted,
            impersonating,
            "renderer token not locked down"
        );
    }
    report.filesystem = locked;
    report.network = locked;
    report.processes = in_job;
    report.privileges =
        locked && report.integrity.is_some_and(|l| l <= 0x1000) && dynamic && images;
    report
}

#[allow(unsafe_code)]
pub(crate) fn integrity_level() -> Option<u32> {
    let token = process_token(TOKEN_QUERY).ok()?;
    let buf = token_info(&token, TokenIntegrityLevel).ok()?;
    // SAFETY: on success the buffer starts with a TOKEN_MANDATORY_LABEL
    // whose SID points into the same buffer.
    let label = unsafe { &*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    // SAFETY: `label.Label.Sid` is a valid SID written by
    // GetTokenInformation; the sub-authority index is read from that SID's
    // own count, so it is in range.
    unsafe {
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(label.Label.Sid, u32::from(count) - 1))
    }
}

#[allow(unsafe_code)]
pub(crate) fn in_job() -> Option<bool> {
    let mut result = 0;
    // SAFETY: pseudo-handle for the current process, a null job handle
    // ("any job") and a valid out-pointer.
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut result) };
    (ok != 0).then_some(result != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_tokens_and_probes_work() {
        Job::new(crate::DEFAULT_RENDERER_MEMORY).expect("job object");
        renderer_token(TokenKind::Lockdown).expect("lockdown token");
        renderer_token(TokenKind::WarmUp).expect("warm-up token");
        // Test processes run at medium integrity (or high when elevated).
        assert!(integrity_level().is_some_and(|l| l >= 0x2000));
        assert!(in_job().is_some());
        assert_eq!(thread_impersonating().ok(), Some(false));
    }

    #[test]
    fn arguments_are_quoted_for_the_msvc_parser() {
        let q = |s: &str| {
            let mut out = Vec::new();
            quote(OsStr::new(s), &mut out);
            String::from_utf16(&out).unwrap()
        };
        assert_eq!(q("plain"), "plain");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(q(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(q(r#"a\"b"#), r#""a\\\"b""#);
    }
}
