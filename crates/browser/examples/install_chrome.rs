use a3s_use_browser::{install_browser, ManagedBrowser};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let status = install_browser(ManagedBrowser::Chrome).await?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}
