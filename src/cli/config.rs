//! Config command - configuration utilities.

use anyhow::Result;

use crate::cli::ConfigAction;
use crate::config::{self, Config};

/// Run the config command.
pub async fn run(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Validate => validate_config().await,
        ConfigAction::Show => show_config().await,
        ConfigAction::Path => show_path().await,
    }
}

async fn validate_config() -> Result<()> {
    let config_path = config::paths::config_file();

    println!();
    println!("Validating configuration...");
    println!("Path: {}", config_path.display());
    println!();

    if !config_path.exists() {
        println!("ERROR: Configuration file not found");
        println!();
        println!("Create a configuration file at:");
        println!("  {}", config_path.display());
        println!();
        println!("Or specify a custom path with --config");
        return Ok(());
    }

    match Config::load() {
        Ok(config) => {
            println!("Configuration is valid.");
            println!();
            println!("Summary:");
            println!("  Instruments: {}", config.instruments.len());
            for inst in &config.instruments {
                println!("    - {} ({:?})", inst.id, inst.vendor);
            }
            println!("  Cloud endpoint: {}", config.cloud.endpoint);
            println!(
                "  Certificate: {}",
                config
                    .cloud
                    .certificate_thumbprint
                    .as_deref()
                    .unwrap_or("(not configured)")
            );
        }
        Err(e) => {
            println!("ERROR: Configuration is invalid");
            println!();
            println!("Details: {}", e);
            println!();
            println!("Fix the configuration and run 'mdqc config validate' again.");
        }
    }

    println!();
    Ok(())
}

async fn show_config() -> Result<()> {
    let config_path = config::paths::config_file();

    if !config_path.exists() {
        println!("Configuration file not found at: {}", config_path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&config_path)?;
    println!("{}", content);

    Ok(())
}

async fn show_path() -> Result<()> {
    let config_path = config::paths::config_file();
    println!("{}", config_path.display());
    Ok(())
}
