// `main.rs` compiles this module tree into the `mpc-node` binary too, but only
// the library and the `mpc-enroll` binary call into `feldman`.
#[allow(dead_code)]
pub mod feldman;
pub mod session_encryption;
