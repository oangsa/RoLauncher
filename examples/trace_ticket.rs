// Verify production launch preparation without opening or closing a Roblox process.
// Uses only this application's DPAPI-protected sessions; never prints secrets.
use rolauncher::{platform, roblox::Roblox, store::Store};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data =
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("No app data")?);
    let directory = rolauncher::store::default_directory(&local_app_data);
    let db = Store::new(directory)?.load()?;
    let saved = db.accounts.first().ok_or("No saved account")?;
    let target = saved.account.target.as_ref().ok_or("No target")?;
    let cookie = platform::unprotect(&saved.encrypted_session)?;
    match Roblox::new()?
        .launch_uri(&cookie, target, "1234567890123456")
        .await
    {
        Ok(uri) => {
            drop(uri);
            println!(
                "Production launch preparation succeeded; ticket and launch URI discarded. No client launched."
            );
        }
        Err(failure) => {
            println!("{}", failure.message);
            std::process::exit(1);
        }
    }
    Ok(())
}
