use std::path::{Path, PathBuf};

const SHA1_HEX_LENGTH: usize = 40;

/// An installed game tree, with a native archive cache only for shared installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDir {
    root: PathBuf,
    native_cache: Option<PathBuf>,
}

impl GameDir {
    pub(super) fn shared(root: PathBuf, native_cache: PathBuf) -> Self {
        Self {
            root,
            native_cache: Some(native_cache),
        }
    }

    /// A caller-supplied game tree that must not use the shared archive cache.
    pub fn standalone(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            native_cache: None,
        }
    }

    /// The private game files prepared for a portable instance.
    pub fn isolated(instance: &Path) -> Self {
        Self::standalone(instance.join(".axl-runtime"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_document(&self, id: &str) -> PathBuf {
        version_document(&self.versions(), id)
    }

    pub fn client_jar(&self, id: &str) -> PathBuf {
        self.versions().join(id).join(format!("{id}.jar"))
    }

    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn asset_object(&self, hash: &str) -> Option<PathBuf> {
        let prefix = hash.get(..2)?;
        if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.assets().join("objects").join(prefix).join(hash))
    }

    pub fn asset_index(&self, id: &str) -> PathBuf {
        self.assets().join("indexes").join(format!("{id}.json"))
    }

    pub fn virtual_assets(&self, id: &str) -> PathBuf {
        self.assets().join("virtual").join(id)
    }

    pub fn legacy_resources(&self) -> PathBuf {
        self.root.join("resources")
    }

    pub fn log_config(&self, id: &str) -> PathBuf {
        self.root.join("log_configs").join(id)
    }

    pub fn version_natives(&self, id: &str) -> PathBuf {
        self.root.join("natives").join(id)
    }

    pub fn natives_lock(&self, id: &str) -> PathBuf {
        self.root
            .join("natives")
            .join(".locks")
            .join(format!("{id}.lock"))
    }

    pub fn install_lock(&self) -> PathBuf {
        self.root.join(".install.lock")
    }

    /// The single location of an archive from a manifest's native-classifier map.
    ///
    /// A shared tree caches it by SHA-1 when the hash is usable. Other trees
    /// and classifiers without a usable hash use the manifest's Maven layout.
    pub fn native_classifier_archive(&self, maven_relative: &Path, sha1: Option<&str>) -> PathBuf {
        if let (Some(cache), Some(sha1)) = (&self.native_cache, sha1)
            && sha1.len() == SHA1_HEX_LENGTH
            && sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return cache.join(format!("{sha1}.jar"));
        }
        self.libraries().join(maven_relative)
    }
}

/// The version document in a supplied versions directory.
pub fn version_document(versions: &Path, id: &str) -> PathBuf {
    versions.join(id).join(format!("{id}.json"))
}
