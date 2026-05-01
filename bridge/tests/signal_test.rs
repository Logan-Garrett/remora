use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The bridge binary should exit cleanly on SIGTERM instead of hanging.
#[test]
#[cfg(unix)]
fn bridge_exits_on_sigterm() {
    let bridge = env!("CARGO_BIN_EXE_remora-bridge");

    // Start the bridge with a non-routable address so it blocks on connect.
    let mut child = Command::new(bridge)
        .arg("ws://192.0.2.1:1/fake")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start bridge");

    // Give it a moment to start
    std::thread::sleep(Duration::from_millis(200));

    // Send SIGTERM
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    // Poll for exit with a timeout
    let start = Instant::now();
    let timeout = Duration::from_secs(3);
    loop {
        match child.try_wait().expect("try_wait failed") {
            Some(_status) => {
                // Exited — test passes
                return;
            }
            None => {
                if start.elapsed() > timeout {
                    child.kill().ok();
                    child.wait().ok();
                    panic!("bridge did not exit within 3 seconds after SIGTERM");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
