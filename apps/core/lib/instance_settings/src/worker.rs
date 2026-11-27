use axlauncher_worker::instance_settings::Reply;
use axlauncher_worker::serve;

use crate::validate;

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_instance_settings_v1() -> i32 {
    serve(|edit, reply, cancel| async move {
        if cancel.is_pending() {
            reply.send(match validate(edit) {
                Ok(edit) => Reply::Validated(edit),
                Err(error) => Reply::Rejected(error),
            })
        } else {
            Err("The settings edit was cancelled.".into())
        }
    })
}
