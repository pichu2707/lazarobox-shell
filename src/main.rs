#[tokio::main]
async fn main() -> std::io::Result<()> {
    lazarobox_shell::runtime::run().await
}
