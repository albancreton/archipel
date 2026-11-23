//! The `bridged` program: the daemon with every addon the library ships. See the library.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    bridged::main(bridge_addons::all()).await
}
