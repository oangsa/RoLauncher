// Prints only verified process identity and boolean ownership; never raw command lines.
use rolauncher::{platform, store::Store};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data =
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("No app data")?);
    let directory = rolauncher::store::default_directory(&local_app_data);
    let db = Store::new(directory)?.load()?;
    for player in platform::players()? {
        let matches = db
            .accounts
            .iter()
            .filter(|saved| saved.account.tracker.as_deref() == Some(&player.tracker))
            .count();
        println!(
            "verified_pid={} creation_time={} saved_tracker_matches={}",
            player.pid, player.creation_time, matches
        );
    }
    Ok(())
}
