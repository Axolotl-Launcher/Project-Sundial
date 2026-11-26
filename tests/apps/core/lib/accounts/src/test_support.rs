use std::path::Path;

pub(crate) struct TempRoot(tempfile::TempDir);

impl TempRoot {
    pub(crate) fn new() -> Self {
        Self(tempfile::tempdir().expect("temporary account data"))
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }
}
