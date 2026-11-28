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
        if self.leader_reaped {
            return group_empty(leader);
        }
        let exited = leader_exited(leader)?;
        if !exited && stop {
            terminate_group(leader)?;
        }
        if !exited {
            return Ok(false);
        }
        terminate_exited_group(leader)?;
        self.leader_reaped = child.try_wait()?.is_some();
        if !self.leader_reaped {
            return Ok(false);
        }
        group_empty(leader)
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
    reason = "The owned leader still identifies the isolated process group."
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

// macOS reports EPERM when the group contains only the unreaped leader and
// any descendants that are already zombies.
fn terminate_exited_group(leader: libc::pid_t) -> io::Result<()> {
    match terminate_group(leader) {
        Err(error) if zombie_group(&error) => Ok(()),
        result => result,
    }
}

#[allow(
    unsafe_code,
    reason = "The leader was reaped and the group was terminated; signal zero is a non-destructive drain probe."
)]
fn group_empty(leader: libc::pid_t) -> io::Result<bool> {
    if unsafe { libc::kill(-leader, 0) } == 0 {
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(true);
    }
    if zombie_group(&error) {
        return Ok(true);
    }
    Err(error)
}

fn zombie_group(error: &io::Error) -> bool {
    cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::EPERM)
}
