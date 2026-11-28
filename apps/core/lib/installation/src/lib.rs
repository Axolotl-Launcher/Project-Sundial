//! Installs game files without depending on desktop state or the download engine.

pub mod loader;
pub mod prepare;
pub mod runtime;
pub mod vanilla;

mod processors;
mod worker;

#[cfg(test)]
use sha1_smol as test_sha1;
#[cfg(test)]
use tempfile as test_tempfile;
