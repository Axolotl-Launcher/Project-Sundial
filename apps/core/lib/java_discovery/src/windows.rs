//! Windows Java discovery from installation locations and bounded exploration.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_launch::JavaMode;
use axlauncher_launch::java_paths::{
    contains_excluded_java_directory, excluded_from_discovery, executable_in, executable_in_home,
};
use axlauncher_storage::WindowsJavaPaths;

use crate::scan::Scanner;

const EXPLORATION_DEPTH: u8 = 6;
const MAX_VERSION_DIRECTORY_LENGTH: usize = 20;
const INSTALLATION_KEYWORDS: &[&str] = &[
    "java",
    "jdk",
    "jre",
    "dragonwell",
    "azul",
    "zulu",
    "oracle",
    "open",
    "amazon",
    "corretto",
    "eclipse",
    "temurin",
    "hotspot",
    "semeru",
    "kona",
    "bellsoft",
];
const EXPLORATION_KEYWORDS: &[&str] = &[
    "environment",
    "env",
    "runtime",
    "x86_64",
    "amd64",
    "arm64",
    "x64",
    "pcl",
    "hmcl",
    "baka",
    "minecraft",
];

pub(super) fn scan(scanner: &mut Scanner<'_>, paths: WindowsJavaPaths) -> Result<(), String> {
    scanner.checkpoint()?;
    let WindowsJavaPaths {
        drives,
        exploration_roots,
        registry_homes,
        working_directory,
        store_runtime_root,
    } = paths;
    scan_program_files(scanner, &exploration_roots)?;
    let roots = exploration_seeds(scanner, exploration_roots, &drives)?;
    explore(scanner, roots)?;
    for home in registry_homes {
        candidate_in_home(scanner, &home)?;
    }
    scanner.checkpoint()?;
    if let Some(root) = working_directory
        && let Some(path) = executable_in(&root, JavaMode::Console)
    {
        scanner.candidate(path)?;
    }
    if let Some(root) = store_runtime_root {
        store_executables(scanner, &root)?;
    }
    scanner.checkpoint()
}

fn scan_program_files(scanner: &mut Scanner<'_>, roots: &[PathBuf]) -> Result<(), String> {
    scanner.conventional_roots(roots.iter().filter(|path| is_program_files(path)).cloned())
}

fn exploration_seeds(
    scanner: &mut Scanner<'_>,
    mut roots: Vec<PathBuf>,
    drives: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    for drive in drives {
        scanner.checkpoint()?;
        for path in children(drive) {
            scanner.checkpoint()?;
            roots.extend((path.is_dir() && is_installation_root(&path)).then_some(path));
        }
    }
    Ok(roots)
}

fn explore(
    scanner: &mut Scanner<'_>,
    roots: impl IntoIterator<Item = PathBuf>,
) -> Result<(), String> {
    let mut queue: VecDeque<_> = roots.into_iter().map(|root| (root, 0)).collect();
    let mut visited = HashSet::new();
    while let Some((path, depth)) = queue.pop_front() {
        scanner.checkpoint()?;
        let Some(directory) = exploration_directory(path) else {
            continue;
        };
        if visited.insert(directory.clone()) {
            scan_children(scanner, &directory, depth, &mut queue)?;
        }
    }
    Ok(())
}

fn exploration_directory(path: PathBuf) -> Option<PathBuf> {
    if excluded_from_discovery(&path) {
        return None;
    }
    let directory = dunce::canonicalize(path).ok()?;
    (!excluded_from_discovery(&directory)).then_some(directory)
}

fn scan_children(
    scanner: &mut Scanner<'_>,
    directory: &Path,
    depth: u8,
    queue: &mut VecDeque<(PathBuf, u8)>,
) -> Result<(), String> {
    for child in children(directory) {
        scanner.checkpoint()?;
        if !child.is_dir() || excluded_from_discovery(&child) {
            continue;
        }
        if let Some(path) = executable_in(&child, JavaMode::Console) {
            scanner.candidate(path)?;
        }
        if depth < EXPLORATION_DEPTH
            && directory_name(&child).is_some_and(|name| may_descend(&name))
        {
            queue.push_back((child, depth + 1));
        }
    }
    Ok(())
}

fn store_executables(scanner: &mut Scanner<'_>, root: &Path) -> Result<(), String> {
    scanner.checkpoint()?;
    let Ok(root) = dunce::canonicalize(root) else {
        return Ok(());
    };
    if excluded_from_discovery(&root) {
        return Ok(());
    }
    visit_directories(scanner, &root, |scan, component| {
        if directory_name(&component).is_some_and(|name| name.starts_with("java-runtime")) {
            store_component(scan, &component)?;
        }
        Ok(())
    })
}

fn store_component(scanner: &mut Scanner<'_>, component: &Path) -> Result<(), String> {
    visit_directories(scanner, component, |scan, architecture| {
        visit_directories(scan, &architecture, |scan, home| {
            candidate_in_home(scan, &home)
        })
    })
}

fn candidate_in_home(scanner: &mut Scanner<'_>, home: &Path) -> Result<(), String> {
    scanner.checkpoint()?;
    if let Some(path) = executable_in_home(home, JavaMode::Console) {
        scanner.candidate(path)?;
    }
    Ok(())
}

fn visit_directories(
    scanner: &mut Scanner<'_>,
    root: &Path,
    mut visit: impl FnMut(&mut Scanner<'_>, PathBuf) -> Result<(), String>,
) -> Result<(), String> {
    scanner.checkpoint()?;
    for path in children(root) {
        scanner.checkpoint()?;
        if path.is_dir() {
            visit(scanner, path)?;
        }
    }
    Ok(())
}

fn children(root: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
}

fn is_program_files(path: &Path) -> bool {
    directory_name(path)
        .is_some_and(|name| matches!(name.as_str(), "program files" | "program files (x86)"))
}

fn is_installation_root(path: &Path) -> bool {
    directory_name(path).is_some_and(|name| {
        INSTALLATION_KEYWORDS
            .iter()
            .any(|keyword| name.contains(keyword))
    })
}

fn directory_name(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_str()?.to_ascii_lowercase())
}

fn may_descend(name: &str) -> bool {
    if contains_excluded_java_directory(name) {
        return false;
    }
    INSTALLATION_KEYWORDS
        .iter()
        .chain(EXPLORATION_KEYWORDS)
        .any(|keyword| name.contains(keyword))
        || version_directory(name)
}

fn version_directory(name: &str) -> bool {
    name.len() <= MAX_VERSION_DIRECTORY_LENGTH
        && name.bytes().any(|byte| byte.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/java_discovery/src/windows.rs"]
mod tests;
