use axlauncher_storage::java_search_roots;
use axlauncher_worker::java_discovery::{Candidate, Command};
use axlauncher_worker::{Cancellation, Responder, serve};

use crate::scan::Scanner;

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_java_discovery_v1() -> i32 {
    serve(|Command::Scan, reply, cancellation| async move { scan_system(reply, &cancellation) })
}

fn scan_system(reply: Responder<Candidate>, cancellation: &Cancellation) -> Result<(), String> {
    let mut emit = |path| reply.send(Candidate::new(path));
    let mut scanner = Scanner::new(cancellation, &mut emit);
    scanner.checkpoint()?;
    scanner.conventional_roots(java_search_roots())?;
    #[cfg(windows)]
    crate::windows::scan(
        &mut scanner,
        axlauncher_storage::WindowsJavaPaths::discover(),
    )?;
    scanner.checkpoint()
}
