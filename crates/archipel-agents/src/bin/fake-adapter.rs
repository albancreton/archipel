//! A program that behaves like an ACP adapter by replaying a recorded session on stdin and stdout.
//!
//! `fake-adapter <transcript.ndjson> [speed]`

use archipel_agents::fake;
use archipel_core::transcript::Transcript;
use archipel_transport::lines;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: fake-adapter <transcript.ndjson> [speed]")?;
    let speed = args.next().map_or(Ok(0.0), |text| text.parse::<f64>())?;
    let transcript = Transcript::parse(&std::fs::read_to_string(path)?)?;
    fake::replay(
        transcript,
        speed,
        lines(tokio::io::stdin(), tokio::io::stdout()),
    )
    .await;
    Ok(())
}
