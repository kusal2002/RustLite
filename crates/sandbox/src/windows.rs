//! Win32 implementation. Every `unsafe` block calls one documented Win32
//! function; handles are owned by RAII wrappers and closed exactly once.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, CreateWellKnownSid, GetSidSubAuthority, GetSidSubAuthorityCount,
    GetTokenInformation, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES, SetTokenInformation,
    TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_PRIVILEGES, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel, WinLowLabelSid,
};
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
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcessToken, ProcessDynamicCodePolicy, ProcessImageLoadPolicy,
    SetProcessMitigationPolicy,
};

use crate::Lockdown;

/// `SE_GROUP_INTEGRITY` (winnt.h): marks the SID as a mandatory label.
const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;

/// An owned kernel handle, closed on drop.
#[derive(Debug)]
struct Handle(HANDLE);

impl Drop for Handle {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid handle this wrapper exclusively owns
        // (constructed only from successful Create/Open calls) and it is
        // closed exactly once, here.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub(crate) struct Job {
    handle: Handle,
}

impl Job {
    #[allow(unsafe_code)]
    pub(crate) fn new(memory_limit: usize) -> io::Result<Self> {
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
        let ok = unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

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
        let ok = unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectBasicUIRestrictions,
                (&raw const ui).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    #[allow(unsafe_code)]
    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        let process = child.as_raw_handle() as HANDLE;
        // SAFETY: the job handle is valid for `self`'s lifetime and
        // `process` is the live process handle owned by `child`, which the
        // caller borrows for the duration of the call.
        let ok = unsafe { AssignProcessToJobObject(self.handle.0, process) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[allow(unsafe_code)]
fn process_token(access: u32) -> io::Result<Handle> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
    // closing; `token` is a valid out-pointer.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(token))
}

#[allow(unsafe_code)]
fn set_low_integrity() -> io::Result<()> {
    let token = process_token(TOKEN_ADJUST_DEFAULT | TOKEN_QUERY)?;
    let mut sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut size = SECURITY_MAX_SID_SIZE;
    // SAFETY: `sid` is a writable buffer of `size` bytes (the documented
    // maximum SID size); no domain SID is needed for a label SID.
    let ok = unsafe {
        CreateWellKnownSid(
            WinLowLabelSid,
            std::ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    // SAFETY: `token` was opened with TOKEN_ADJUST_DEFAULT; `label` points
    // at `sid`, which outlives the call; the length covers the label struct
    // plus the SID it references, as the API requires.
    let ok = unsafe {
        SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            (std::mem::size_of::<TOKEN_MANDATORY_LABEL>() + size as usize) as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn remove_privileges() -> io::Result<()> {
    let token = process_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;
    // SAFETY: with DisableAllPrivileges = TRUE the new-state pointer may be
    // null and no previous state is requested (null pointers, zero length).
    let ok = unsafe {
        AdjustTokenPrivileges(
            token.0,
            1,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Sets a mitigation policy whose structure is a single `u32` of flags
/// (PROCESS_MITIGATION_*_POLICY are 4-byte unions of bitfields).
#[allow(unsafe_code)]
fn mitigation(policy: i32, flags: u32) -> io::Result<()> {
    // SAFETY: the buffer is a u32 the size of the policy structure, valid
    // for the duration of the call.
    let ok = unsafe {
        SetProcessMitigationPolicy(
            policy,
            (&raw const flags).cast(),
            std::mem::size_of::<u32>(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn lockdown() -> Lockdown {
    let mut report = Lockdown::default();
    // Order matters: privileges first (lowering integrity first can make
    // later token changes fail), mitigations last.
    match remove_privileges() {
        Ok(()) => report.privileges_removed = true,
        Err(e) => tracing::warn!(error = %e, "could not remove renderer privileges"),
    }
    match set_low_integrity() {
        Ok(()) => report.low_integrity = true,
        Err(e) => tracing::warn!(error = %e, "could not lower renderer integrity"),
    }
    // PROCESS_MITIGATION_DYNAMIC_CODE_POLICY: ProhibitDynamicCode (bit 0).
    match mitigation(ProcessDynamicCodePolicy, 1) {
        Ok(()) => report.dynamic_code_blocked = true,
        Err(e) => tracing::warn!(error = %e, "could not prohibit dynamic code"),
    }
    // PROCESS_MITIGATION_IMAGE_LOAD_POLICY: NoRemoteImages (bit 0),
    // NoLowMandatoryLabelImages (bit 1), PreferSystem32Images (bit 2).
    match mitigation(ProcessImageLoadPolicy, 0b111) {
        Ok(()) => report.image_load_restricted = true,
        Err(e) => tracing::warn!(error = %e, "could not restrict image loads"),
    }
    report
}

#[allow(unsafe_code)]
pub(crate) fn integrity_level() -> Option<u32> {
    let token = process_token(TOKEN_QUERY).ok()?;
    let mut buf = [0u8; 256];
    let mut len = 0u32;
    // SAFETY: `buf` is a writable buffer of the given length (the label
    // plus one SID needs far less than 256 bytes); `len` is a valid
    // out-pointer.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut len,
        )
    };
    if ok == 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a TOKEN_MANDATORY_LABEL;
    // `read_unaligned` copes with the byte array's alignment.
    let label: TOKEN_MANDATORY_LABEL =
        unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()) };
    // SAFETY: `label.Label.Sid` points into `buf` at a valid SID written
    // by GetTokenInformation; the sub-authority count and index are read
    // from that SID, so the index is in range.
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
    fn job_creation_and_probes_work() {
        Job::new(crate::DEFAULT_RENDERER_MEMORY).expect("job object");
        // Test processes run at medium integrity (or high when elevated).
        assert!(integrity_level().is_some_and(|l| l >= 0x2000));
        assert!(in_job().is_some());
    }
}
