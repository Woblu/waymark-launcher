//! Runs a game where nobody can see it, on a Windows desktop of its own, so a check
//! after an update can't take focus from whatever the player is doing. Rust's process
//! tools can't choose a desktop, so this talks to Windows directly.
//!
//! The game runs at low priority, in a job that ends it if the launcher dies, so a
//! crash here can't leave an invisible game running.

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, CreateDesktopW, HDESK};
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    CreateProcessW, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOW,
    WaitForSingleObject,
};

use crate::error::{Error, Result};

const DESKTOP: &str = "WaymarkCheck";
const DESKTOP_PATH: &str = "WinSta0\\WaymarkCheck";
const GENERIC_ALL: u32 = 0x1000_0000;

pub struct HiddenGame {
    process: HANDLE,
    job: HANDLE,
    desktop: HDESK,
}

// The handles are only numbers, and Windows lets any thread use them.
unsafe impl Send for HiddenGame {}
unsafe impl Sync for HiddenGame {}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn failed(what: &str) -> Error {
    Error::Other(format!("{what}: {}", std::io::Error::last_os_error()))
}

/// Quotes an argument the way programs on Windows split their command line.
fn quoted(argument: &OsStr, line: &mut Vec<u16>) {
    let plain = !argument.is_empty()
        && !argument
            .encode_wide()
            .any(|c| c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'"'));
    if plain {
        line.extend(argument.encode_wide());
        return;
    }
    line.push(u16::from(b'"'));
    let mut slashes = 0;
    for c in argument.encode_wide() {
        if c == u16::from(b'\\') {
            slashes += 1;
            continue;
        }
        let doubled = if c == u16::from(b'"') {
            slashes * 2 + 1
        } else {
            slashes
        };
        line.extend(std::iter::repeat_n(u16::from(b'\\'), doubled));
        slashes = 0;
        line.push(c);
    }
    line.extend(std::iter::repeat_n(u16::from(b'\\'), slashes * 2));
    line.push(u16::from(b'"'));
}

/// Windows wants these sorted, each ended by a nul, with one more at the end.
fn environment(extra: &[(OsString, OsString)]) -> Vec<u16> {
    let mut all: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| !extra.iter().any(|(key, _)| key.eq_ignore_ascii_case(name)))
        .chain(extra.iter().cloned())
        .collect();
    all.sort_by_key(|(name, _)| name.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (name, value) in all {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

impl HiddenGame {
    /// Starts `program` on the hidden desktop with its output going to `log`.
    pub fn start(
        program: &Path,
        arguments: &[String],
        environ: &[(OsString, OsString)],
        folder: &Path,
        log: &Path,
    ) -> Result<HiddenGame> {
        let mut line = Vec::new();
        quoted(program.as_os_str(), &mut line);
        for argument in arguments {
            line.push(u16::from(b' '));
            quoted(OsStr::new(argument), &mut line);
        }
        line.push(0);
        let block = environment(environ);
        let folder = wide(folder.as_os_str());
        let log = wide(log.as_os_str());
        let name = wide(OsStr::new(DESKTOP));
        let mut seat = wide(OsStr::new(DESKTOP_PATH));

        // Everything Windows hands out here is closed below or by `Drop`.
        unsafe {
            let desktop = CreateDesktopW(name.as_ptr(), null(), null(), 0, GENERIC_ALL, null());
            if desktop.is_null() {
                return Err(failed("couldn't make the hidden desktop"));
            }
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                CloseDesktop(desktop);
                return Err(failed("couldn't make a job for the check"));
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            let inherit = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            let output = CreateFileW(
                log.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                &raw const inherit,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            );
            if output == INVALID_HANDLE_VALUE {
                CloseHandle(job);
                CloseDesktop(desktop);
                return Err(failed("couldn't open the check's log"));
            }

            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = size_of::<STARTUPINFOW>() as u32;
            startup.lpDesktop = seat.as_mut_ptr();
            startup.dwFlags = STARTF_USESTDHANDLES;
            startup.hStdOutput = output;
            startup.hStdError = output;
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let started = CreateProcessW(
                null(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                BELOW_NORMAL_PRIORITY_CLASS
                    | CREATE_NO_WINDOW
                    | CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT,
                block.as_ptr().cast::<c_void>(),
                folder.as_ptr(),
                &raw const startup,
                &raw mut info,
            );
            CloseHandle(output);
            if started == 0 {
                CloseHandle(job);
                CloseDesktop(desktop);
                return Err(failed("couldn't start the game for the check"));
            }
            AssignProcessToJobObject(job, info.hProcess);
            ResumeThread(info.hThread);
            CloseHandle(info.hThread);
            Ok(HiddenGame {
                process: info.hProcess,
                job,
                desktop,
            })
        }
    }

    pub fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 }
    }

    pub fn stop(&self) {
        unsafe {
            TerminateJobObject(self.job, 0);
        }
    }
}

impl Drop for HiddenGame {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.process);
            CloseHandle(self.job);
            CloseDesktop(self.desktop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_for(argument: &str) -> String {
        let mut line = Vec::new();
        quoted(OsStr::new(argument), &mut line);
        String::from_utf16(&line).unwrap()
    }

    #[test]
    fn arguments_survive_windows_splitting_them() {
        assert_eq!(line_for("plain"), "plain");
        assert_eq!(line_for(""), "\"\"");
        assert_eq!(line_for("two words"), "\"two words\"");
        assert_eq!(line_for("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(line_for("C:\\a b\\"), "\"C:\\a b\\\\\"");
        assert_eq!(line_for("a\\b"), "a\\b");
    }

    #[test]
    fn the_environment_is_sorted_and_ends_twice() {
        let block = environment(&[(OsString::from("WAYMARK_TEST"), OsString::from("yes"))]);
        let text = String::from_utf16(&block).unwrap();
        assert!(text.contains("WAYMARK_TEST=yes\0"));
        assert!(text.ends_with("\0\0"));
    }
}
