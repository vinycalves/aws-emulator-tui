//! AWS Emulator TUI — a terminal console for a local AWS Emulator.

mod app;
mod theme;
mod ui;

use anyhow::Context;
use emu_core::{config, services, EmulatorClient};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE: &str = "\
AWS Emulator TUI — browse and drive a local AWS Emulator from the terminal

USAGE:
    emutui [OPTIONS]

OPTIONS:
    --endpoint <URL>    Emulator endpoint (default: $EMULATOR_ENDPOINT or https://127.0.0.1:9297)
    --region <REGION>   SigV4 region (default: $AWS_REGION or us-east-1)
    --check             Print a connectivity report and exit
    --version           Print the version
    -h, --help          Print this help

KEYS:
    1-4 / Tab  switch tab        ?          help
    a          all services      q          quit
    Explorer:  enter inspect · c create · d delete · / filter
    Console:   s service · o operation · e edit · Ctrl+S send
    Emulator:  s start · x stop · R restart
";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut endpoint: Option<String> = None;
    let mut region: Option<String> = None;
    let mut check = false;

    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--endpoint" | "-e" => {
                endpoint = Some(args.next().context("--endpoint needs a value")?);
            }
            "--region" => {
                region = Some(args.next().context("--region needs a value")?);
            }
            "--check" => check = true,
            "--version" | "-v" => {
                println!("emutui {VERSION}");
                return Ok(());
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument `{other}`; try --help"),
        }
    }

    if let Some(endpoint) = &endpoint {
        std::env::set_var("EMULATOR_ENDPOINT", endpoint);
    }
    if let Some(region) = &region {
        std::env::set_var("AWS_REGION", region);
    }

    if check {
        return run_check().await;
    }

    let config = config::discover();
    let app = app::App::new(config)?;
    app.run().await
}

async fn run_check() -> anyhow::Result<()> {
    let config = config::discover();
    println!("emutui {VERSION}");
    println!("endpoint      {}", config.endpoint);
    println!("region        {}", config.region);
    println!("credentials   {}", config.credentials_source.label());
    println!("config        {}", config.paths.config.display());
    println!("ca            {}", config.paths.ca.display());
    for warning in &config.warnings {
        println!("warning       {warning}");
    }

    let client = EmulatorClient::new(&config)?;
    if let Err(error) = client.health().await {
        println!("health        FAILED — {error}");
        anyhow::bail!("emulator is not reachable");
    }
    println!("health        ok");

    match services::caller_identity(&client).await {
        Ok(identity) => println!("identity      {identity}"),
        Err(error) => println!("identity      error — {}", error.message),
    }

    println!();
    println!("{:<20} {:<8} DETAIL", "SERVICE", "STATUS");
    let mut failures = 0;
    for service in services::CURATED {
        match services::probe(&client, service.key).await {
            Ok(()) => println!("{:<20} {:<8} ok", service.key, "up"),
            Err(error) => {
                failures += 1;
                println!(
                    "{:<20} {:<8} {} {}",
                    service.key,
                    "error",
                    error.status_label(),
                    error.message.lines().next().unwrap_or_default()
                );
            }
        }
    }
    println!();
    println!(
        "{} curated services checked, {} failed",
        services::CURATED.len(),
        failures
    );
    Ok(())
}
