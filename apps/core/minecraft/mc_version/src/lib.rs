//! Classifies upstream Minecraft version conventions without I/O.

#![forbid(unsafe_code)]

mod java;
mod maven;

pub use axlauncher_spi::mc_version::{JavaVersion, JavaVersionKind};
pub use java::{BuiltinJavaVersionParser, classify_java};
pub use maven::{MavenArtifact, MavenBuild, MavenBuilds, MavenTarget};
