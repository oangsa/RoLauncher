// Explicit local-file diagnostic: outputs signal counts, never raw log contents.
use rolauncher::logs::{Signal, Tail};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("A local log path is required")?;
    let mut tail = Tail::new(path.into(), false)?;
    let mut counts = [0u64; 5];
    loop {
        let before = tail.offset;
        for signal in tail.read()? {
            counts[match signal {
                Signal::Connected => 0,
                Signal::Disconnected => 1,
                Signal::ConnectionFailed | Signal::SessionLost(_) => 2,
                Signal::TargetUnavailable => 3,
                Signal::PermissionDenied => 4,
            }] += 1;
        }
        if tail.offset == before {
            break;
        }
    }
    println!(
        "connected={} disconnected={} connection_failed={} unavailable={} permission={}",
        counts[0], counts[1], counts[2], counts[3], counts[4]
    );
    Ok(())
}
