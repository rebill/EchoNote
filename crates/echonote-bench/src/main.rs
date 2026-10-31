//! EchoNote ASR benchmarks. See `docs/V1_0_0_DESKTOP_APP_TECH_DESIGN.md` for how the numbers
//! feed the M0 gates.

mod audio;
mod eval;
mod live;
mod metrics;
mod model_arg;
mod sys_metrics;
mod text;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "echonote-bench", about = "EchoNote ASR benchmarks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decode one WAV file and print the text and timing.
    Smoke(eval::SmokeArgs),
    /// Decode every utterance in a JSONL manifest; report error rate, RTF and resources.
    Eval(eval::EvalArgs),
    /// Simulate the live pipeline (VAD, previews, finals) over one or two tracks.
    Live(live::LiveArgs),
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Smoke(args) => eval::smoke(args),
        Command::Eval(args) => eval::run(args),
        Command::Live(args) => live::run(args),
    }
}
