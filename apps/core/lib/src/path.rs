use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(unix)]
pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().serialize(serializer)
}

#[cfg(unix)]
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    use std::os::unix::ffi::OsStringExt;
    Vec::<u8>::deserialize(deserializer).map(|path| std::ffi::OsString::from_vec(path).into())
}

#[cfg(windows)]
pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .collect::<Vec<_>>()
        .serialize(serializer)
}

#[cfg(windows)]
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    use std::os::windows::ffi::OsStringExt;
    Vec::<u16>::deserialize(deserializer).map(|path| std::ffi::OsString::from_wide(&path).into())
}
