//! macOS only: `/usr/bin/sandbox-exec` must really confine a program on this
//! machine. Set `LAZAROBOX_REQUIRE_JAIL=1` to fail instead of skipping, so a
//! CI job that is meant to have Seatbelt cannot go green by testing nothing.

#![cfg(target_os = "macos")]

use lazarobox_shell::runtime::jail::seatbelt_probe;

#[test]
fn sandbox_exec_confines_the_probe_programs() {
    let required = std::env::var_os("LAZAROBOX_REQUIRE_JAIL").is_some_and(|v| v == "1");
    match seatbelt_probe() {
        Ok(()) => {}
        Err(fault) if required => {
            panic!("LAZAROBOX_REQUIRE_JAIL=1 but the probe failed: {fault:?}")
        }
        Err(fault) => eprintln!("skipped: the macOS probe failed ({fault:?})"),
    }
}
