//! Owns Minecraft account documents, sessions and authentication.

mod accounts;
mod worker;

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/accounts/src/test_support.rs"]
mod test_support;
