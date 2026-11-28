use std::rc::Rc;

use axlauncher_worker::{modrinth::Command, serve};

use crate::catalog::Catalog;

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_modrinth_v1() -> i32 {
    let catalog = Rc::new(Catalog::desktop());
    serve(move |Command::V1(operation), reply, cancel| {
        let catalog = Rc::clone(&catalog);
        async move {
            tokio::select! {
                biased;
                () = cancel.cancelled() => Err("The Modrinth request was cancelled.".into()),
                result = catalog.lookup(operation) => reply.send(result),
            }
        }
    })
}
