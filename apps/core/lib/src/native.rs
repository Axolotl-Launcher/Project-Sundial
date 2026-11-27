use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use axlauncher_storage::{DataDir, ScratchSpace, WorkerLibrary};

pub(crate) const WORKER_ARGUMENT: &str = "--axlauncher-worker";

#[derive(Clone, Copy)]
pub struct Image {
    kind: WorkerLibrary,
    bytes: &'static [u8],
    symbol: &'static [u8],
}

impl Image {
    pub(crate) const fn new(kind: WorkerLibrary, bytes: &'static [u8]) -> Self {
        let symbol: &[u8] = match kind {
            WorkerLibrary::Downloads => b"axlauncher_downloads_v1\0",
            WorkerLibrary::Accounts => b"axlauncher_accounts_v1\0",
        };
        Self {
            kind,
            bytes,
            symbol,
        }
    }

    pub(crate) fn matches(self, kind: &OsStr) -> bool {
        kind == self.kind.name()
    }

    pub(super) fn extract(self, data: &DataDir) -> io::Result<(tempfile::TempDir, PathBuf)> {
        let scratch = data.scratch(ScratchSpace::Worker)?;
        let path = self.kind.path(scratch.path());
        let mut file = std::fs::File::create_new(&path)?;
        file.write_all(self.bytes)?;
        Ok((scratch, path))
    }

    pub(super) fn spawn(self, executable: &Path, path: &Path) -> io::Result<Child> {
        let mut command = Command::new(executable);
        command.args([
            OsStr::new(WORKER_ARGUMENT),
            OsStr::new(self.kind.name()),
            path.as_os_str(),
        ]);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command
            .env_remove("CURSEFORGE_API_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                io::Error::other(format!(
                    "could not launch the worker executable: {error}; restart the launcher if it was updated"
                ))
            })
    }
}

pub(crate) fn run(image: Image, path: &Path) -> io::Result<i32> {
    let mut options = std::fs::File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    if !path.is_absolute()
        || path
            .parent()
            .is_none_or(|parent| image.kind.path(parent) != path)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid worker library path",
        ));
    }
    let mut file = options.open(path)?;
    verify(image, &mut file)?;
    let result = invoke(image, path);
    drop(file);
    result
}

fn verify(image: Image, file: &mut impl io::Read) -> io::Result<()> {
    let mut buffer = [0; 16 * 1024];
    for expected in image.bytes.chunks(buffer.len()) {
        file.read_exact(&mut buffer[..expected.len()])
            .map_err(verify_read_error)?;
        if buffer[..expected.len()] != *expected {
            return Err(invalid_worker_image());
        }
    }
    if file.read(&mut buffer[..1])? != 0 {
        return Err(invalid_worker_image());
    }
    Ok(())
}

fn verify_read_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        invalid_worker_image()
    } else {
        error
    }
}

fn invalid_worker_image() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "worker library does not match the embedded image; restart the launcher",
    )
}

#[allow(
    unsafe_code,
    reason = "Only the verified, bundled worker image can enter its versioned C ABI."
)]
fn invoke(image: Image, path: &Path) -> io::Result<i32> {
    let loaded;
    #[cfg(windows)]
    {
        use libloading::os::windows::{LOAD_LIBRARY_SEARCH_SYSTEM32, Library};
        loaded = unsafe { Library::load_with_flags(path, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map(libloading::Library::from);
    }
    #[cfg(unix)]
    {
        loaded = unsafe { libloading::Library::new(path) };
    }
    let library = loaded.map_err(|error| {
        io::Error::other(format!(
            "could not load the bundled library from the launcher cache: {error}"
        ))
    })?;
    let entry =
        unsafe { library.get::<extern "C" fn() -> i32>(image.symbol) }.map_err(io::Error::other)?;
    let status = std::thread::scope(|scope| scope.spawn(move || entry()).join());
    Ok(if library.close().is_ok() {
        status.unwrap_or(1)
    } else {
        1
    })
}
