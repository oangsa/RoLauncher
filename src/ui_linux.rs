pub fn message(text: &str) {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    eprintln!("RoLauncher: {text}");
    // Menu launches have no terminal. Match Windows' visible error feedback using
    // the same GTK runtime required by the ephemeral sign-in browser.
    if let Ok(mut child) = Command::new("python3")
        .args(["-c", include_str!("../scripts/message-linux.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

pub fn run(
    engine: crate::engine::Engine,
    runtime: tokio::runtime::Handle,
    port: u16,
) -> Result<(), String> {
    crate::desktop::run(engine, runtime, port)
}
