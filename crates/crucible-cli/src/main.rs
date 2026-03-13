use clap::Parser;

#[derive(Parser)]
#[command(name = "crucible", about = "CLI for interacting with Crucible")]
struct Cli {
    /// Crucible API server URL
    #[arg(
        long,
        env = "CRUCIBLE_API_URL",
        default_value = "http://localhost:8080"
    )]
    api_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Submit a job
    Submit,
    /// Get job status
    Status {
        /// Job ID
        job_id: String,
    },
    /// Stream job logs
    Logs {
        /// Job ID
        job_id: String,
        /// Follow log output
        #[arg(long)]
        follow: bool,
    },
    /// Kill a running job
    Kill {
        /// Job ID
        job_id: String,
    },
    /// List jobs
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Submit => {
            println!("submit not yet implemented");
        }
        Commands::Status { job_id } => {
            println!("status for {job_id}: not yet implemented");
        }
        Commands::Logs { job_id, follow: _ } => {
            println!("logs for {job_id}: not yet implemented");
        }
        Commands::Kill { job_id } => {
            println!("kill {job_id}: not yet implemented");
        }
        Commands::List => {
            println!("list: not yet implemented");
        }
    }

    Ok(())
}
