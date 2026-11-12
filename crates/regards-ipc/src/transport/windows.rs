//! Windows named-pipe transport.
#![allow(unsafe_code)]

use super::Stream;
use crate::endpoint::Endpoint;
use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// How long a client waits while all pipe instances are busy.
const BUSY_WAIT: Duration = Duration::from_secs(5);

pub struct Listener {
    name: String,
    security: PipeSecurity,
    next: NamedPipeServer,
}

impl Listener {
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let security = PipeSecurity::current_user_only()?;
        let name = endpoint.pipe_name().to_owned();
        // The first instance must not already exist: this prevents another
        // process from squatting on the name before the daemon starts.
        let next = create_instance(&name, &security, true)?;
        Ok(Self { name, security, next })
    }

    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.next.connect().await?;
        let fresh = create_instance(&self.name, &self.security, false)?;
        let connected = std::mem::replace(&mut self.next, fresh);
        Ok(Box::new(connected))
    }
}

pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        match ClientOptions::new().open(endpoint.pipe_name()) {
            Ok(client) => {
                // Before anything is sent: the name alone does not say who listens.
                check_owner(&client)?;
                return Ok(Box::new(client));
            }
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Refuses a pipe that another user created under the daemon's name.
///
/// The daemon's pipe lets only its own user in, but nothing stops another
/// user of this computer from creating a pipe with the same name while no
/// daemon runs, and waiting for a client to talk to it. A pipe belongs to
/// whoever created it, and only an administrator can give it away, so the
/// owner tells the two apart: this user, or Administrators or the system
/// (the owner Windows gives what an elevated process creates; either could
/// read this user's mail anyway).
fn check_owner(client: &NamedPipeClient) -> io::Result<()> {
    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: allocated by GetSecurityInfo; freed once.
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    let mut owner: PSID = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: the handle is open (it was opened for reading, which includes
    // reading its security); `owner` points into `descriptor`, a LocalAlloc'd
    // buffer owned by `Descriptor` below.
    let status = unsafe {
        GetSecurityInfo(
            client.as_raw_handle().cast(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor = Descriptor(descriptor);
    if owner.is_null() {
        return Err(not_ours());
    }

    let user = current_user()?;
    // SAFETY: `owner` is valid while `_descriptor` lives, and the user's SID
    // while `user` does.
    let trusted = unsafe {
        EqualSid(owner, token_user(&user).User.Sid) != 0
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
            || IsWellKnownSid(owner, WinLocalSystemSid) != 0
    };
    if trusted { Ok(()) } else { Err(not_ours()) }
}

fn not_ours() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the daemon's pipe belongs to another user of this computer; not connecting to it",
    )
}

fn create_instance(name: &str, security: &PipeSecurity, first: bool) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: `security.attributes()` points to a SECURITY_ATTRIBUTES whose
    // descriptor is owned by `security` and outlives this call.
    unsafe { options.create_with_security_attributes_raw(name, security.attributes()) }
}

/// A security descriptor that grants full access to the current user's SID only.
struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: Box<SECURITY_ATTRIBUTES>,
}

// SAFETY: the descriptor is immutable after creation and only read by the OS
// when pipe instances are created; it is freed exactly once in `Drop`.
unsafe impl Send for PipeSecurity {}
// SAFETY: see `Send`; shared access never mutates the descriptor.
unsafe impl Sync for PipeSecurity {}

impl PipeSecurity {
    fn current_user_only() -> io::Result<Self> {
        let sid = current_user_sid()?;
        // D:P = protected DACL (no inherited entries); A;;GA = allow generic all.
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})").encode_utf16().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `sddl` is NUL-terminated UTF-16; `descriptor` receives a
        // LocalAlloc'd buffer that we free in `Drop`.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = Box::new(SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        });
        Ok(Self { descriptor, attributes })
    }

    fn attributes(&self) -> *mut c_void {
        std::ptr::from_ref::<SECURITY_ATTRIBUTES>(&self.attributes)
            .cast_mut()
            .cast()
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: `descriptor` was allocated by
        // ConvertStringSecurityDescriptorToSecurityDescriptorW and is freed once.
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}

/// The current process's user (a `TOKEN_USER` and the SID it points to), in
/// storage aligned for it.
fn current_user() -> io::Result<Vec<u64>> {
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcessToken and is closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    let mut handle: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `handle` receives a
    // token handle owned by `Token` below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(handle);

    let mut needed = 0u32;
    // SAFETY: a size query with a null buffer; failure is expected and only
    // `needed` is used.
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 storage keeps the TOKEN_USER structure correctly aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buffer` is at least `needed` bytes and suitably aligned.
    let ok = unsafe { GetTokenInformation(token.0, TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

/// The `TOKEN_USER` at the start of what [`current_user`] returned.
fn token_user(buffer: &[u64]) -> &TOKEN_USER {
    // SAFETY: `current_user` filled the buffer with a valid, aligned
    // TOKEN_USER, whose SID pointer stays valid as long as the buffer.
    unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() }
}

/// The current process user's SID in string form, for example `S-1-5-21-…`.
fn current_user_sid() -> io::Result<String> {
    let buffer = current_user()?;
    let user = token_user(&buffer);

    let mut wide: *mut u16 = null_mut();
    // SAFETY: `user.User.Sid` is valid while `buffer` lives; `wide` receives a
    // LocalAlloc'd NUL-terminated string freed below.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut wide) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `wide` is NUL-terminated UTF-16 produced by the OS.
    let sid = unsafe {
        let len = (0..).take_while(|&i| *wide.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(wide, len))
    };
    // SAFETY: allocated by ConvertSidToStringSidW; freed once.
    unsafe {
        LocalFree(wide.cast());
    }
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_user_sid_has_sid_format() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "unexpected SID: {sid}");
    }
}
