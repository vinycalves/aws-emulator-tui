//! End-to-end smoke test against a running emulator.
//!
//! ```bash
//! HOME=$EMULATOR_HOME cargo run -p emu-core --example smoke
//! ```

use emu_core::{config, services, EmulatorClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::discover();
    println!("endpoint:            {}", config.endpoint);
    println!("region:              {}", config.region);
    println!("credentials:         {}", config.credentials_source.label());
    println!(
        "ca:                  {:?}",
        config.ca_pem.as_ref().map(Vec::len)
    );
    for warning in &config.warnings {
        println!("warning:             {warning}");
    }

    let client = EmulatorClient::new(&config)?;
    client.health().await.map_err(anyhow::Error::msg)?;
    println!("health:              ok");

    match services::caller_identity(&client).await {
        Ok(identity) => println!("identity:            {identity}"),
        Err(error) => println!("identity:            ERROR {}", error.message),
    }

    let mut failed = 0;
    for service in services::CURATED {
        match services::probe(&client, service.key).await {
            Ok(()) => println!("{:20} ok", service.key),
            Err(error) => {
                failed += 1;
                println!(
                    "{:20} {} {}",
                    service.key,
                    error.status_label(),
                    error.message.lines().next().unwrap_or_default()
                );
            }
        }
    }

    println!("\n{} curated services failed", failed);
    Ok(())
}
