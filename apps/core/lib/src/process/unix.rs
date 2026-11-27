//! Keep the group leader waitable until the last terminating signal has been sent.

use std::io;
use std::process::Child;

#[derive(Default)]
pub(super) struct Tree {
    leader_reaped: bool,
}

impl Tree {
    pub fn new() -> io::Result<Self> {
        Ok(Self::default())
    }

    pub fn attach(&mut self, child: &Child) -> io::Result<()> {
        group_id(child).map(|_| ())
    }

    pub fn poll_exit(&mut self, child: &mut Child, stop: bool) -> io::Result<bool> {
        let leader = group_id(child)?;
        if !self.leader_reaped {
            if !leader_exited(leader)? {
                if stop {
                    terminate_group(leader)?;
                }
                return Ok(false);
            }
            terminate_group(leader)?;
            self.leader_reaped = child.try_wait()?.is_some();
        }
        if self.leader_reaped {
            return group_empty(leader);
        }
        Ok(false)
    }
}

fn group_id(child: &Child) -> io::Result<libc::pid_t> {
    libc::pid_t::try_from(child.id())
        .ok()
        .filter(|&id| id > 1)
        .ok_or_else(|| io::Error::other("invalid worker process group"))
}

#[allow(
    unsafe_code,
    reason = "waitid observes this child without consuming its wait status or PID."
)]
fn leader_exited(leader: libc::pid_t) -> io::Result<bool> {
    let mut status = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            leader as libc::id_t,
            status.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { status.assume_init().si_pid() } == leader)
}

#[allow(
    unsafe_code,
    reason = "The owned, unreaped leader pins the already isolated process group."
)]
fn terminate_group(leader: libc::pid_t) -> io::Result<()> {
    if unsafe { libc::kill(-leader, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error)
}

#[allow(
    unsafe_code,
    reason = "The group was terminated while its leader was unreaped; signal zero is a non-destructive drain probe."
)]
fn group_empty(leader: libc::pid_t) -> io::Result<bool> {
    if unsafe { libc::kill(-leader, 0) } == 0 {
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(true);
    }
    Err(error)
}
