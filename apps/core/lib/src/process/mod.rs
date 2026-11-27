//! Process lifecycle for ordinary workers and workers that start child writers.

use std::io;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout};

use crate::Image;

#[cfg(unix)]
#[path = "unix.rs"]
mod containment;
#[cfg(windows)]
#[path = "windows.rs"]
mod containment;

pub(super) struct Process {
    child: Child,
    tree: Option<containment::Tree>,
}

impl Process {
    pub fn start(
        image: Image,
        executable: &Path,
        path: &Path,
    ) -> io::Result<(Self, io::Result<()>)> {
        let mut tree = image
            .contains_writers()
            .then(containment::Tree::new)
            .transpose()?;
        let child = image.spawn(executable, path)?;
        let setup = tree.as_mut().map_or(Ok(()), |tree| tree.attach(&child));
        Ok((Self { child, tree }, setup))
    }

    /// Only a confirmed exit releases the process's image or any writer lock.
    pub fn poll_exit(&mut self, deadline_expired: bool) -> io::Result<bool> {
        if let Some(tree) = &mut self.tree {
            return tree.poll_exit(&mut self.child, deadline_expired);
        }
        single_exit(&mut self.child, deadline_expired)
    }

    pub fn check_exit(&mut self) -> io::Result<()> {
        if self.tree.is_some()
            && !self
                .child
                .try_wait()?
                .is_some_and(|status| status.success())
        {
            return Err(io::Error::other(
                "the installation worker did not exit cleanly",
            ));
        }
        Ok(())
    }

    pub fn open_pipes(&mut self) -> (ChildStdin, ChildStdout) {
        let input = self.child.stdin.take().expect("piped worker input");
        let output = self.child.stdout.take().expect("piped worker output");
        (input, output)
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }
}

fn single_exit(child: &mut Child, stop: bool) -> io::Result<bool> {
    if child.try_wait()?.is_some() {
        return Ok(true);
    }
    if stop {
        child.kill()?;
    }
    Ok(false)
}
