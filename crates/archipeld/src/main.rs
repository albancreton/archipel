//! The `archipeld` program: the daemon with every addon the library ships. See the library.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    archipeld::main(archipel_addons::all()).await
}
