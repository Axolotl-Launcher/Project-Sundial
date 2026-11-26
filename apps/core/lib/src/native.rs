use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use axlauncher_storage::{DataDir, WorkerLibrary};

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
        let scratch = data.scratch()?;
        let path = self.kind.path(scratch.path());
        let mut file = std::fs::File::create_new(&path)?;
        file.write_all(self.bytes)?;
        Ok((scratch, path))
    }

    pub(super) fn command(self, executable: &Path, path: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(executable);
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
    }
}

pub(crate) fn run(image: Image, path: &Path) -> io::Result<i32> {
    use std::io::Read;

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
    let file = options.open(path)?;
    let mut content = file.try_clone()?.take(image.bytes.len() as u64 + 1);
    let mut buffer = [0; 16 * 1024];
    for expected in image.bytes.chunks(buffer.len()) {
        content.read_exact(&mut buffer[..expected.len()])?;
        if buffer[..expected.len()] != *expected {
            return Err(invalid_worker_image());
        }
    }
    if content.read(&mut buffer[..1])? != 0 {
        return Err(invalid_worker_image());
    }
    let result = invoke(image, path);
    drop(file);
    result
}

fn invalid_worker_image() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "worker library does not match the embedded image",
    )
}

#[allow(
    unsafe_code,
    reason = "Only the verified, bundled worker image can enter its versioned C ABI."
)]
fn invoke(image: Image, path: &Path) -> io::Result<i32> {
    let library;
    #[cfg(windows)]
    {
        use libloading::os::windows::{LOAD_LIBRARY_SEARCH_SYSTEM32, Library};
        library = unsafe { Library::load_with_flags(path, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map(libloading::Library::from)
            .map_err(io::Error::other)?;
    }
    #[cfg(unix)]
    {
        library = unsafe { libloading::Library::new(path) }.map_err(io::Error::other)?;
    }
    let entry =
        unsafe { library.get::<extern "C" fn() -> i32>(image.symbol) }.map_err(io::Error::other)?;
    let result = std::thread::scope(|scope| scope.spawn(move || entry()).join())
        .map_err(|_| io::Error::other("worker entry thread stopped"))?;
    library.close().map_err(io::Error::other)?;
    Ok(result)
}
