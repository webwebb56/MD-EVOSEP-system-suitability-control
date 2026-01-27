//! Baseline command - manage baselines.

use anyhow::Result;
use std::io::{self, Write};

use crate::cli::BaselineAction;
use crate::config::Config;

/// Run the baseline command.
pub async fn run(action: BaselineAction) -> Result<()> {
    match action {
        BaselineAction::List { instrument } => list_baselines(instrument).await,
        BaselineAction::Show { baseline_id } => show_baseline(&baseline_id).await,
        BaselineAction::Reset { instrument, confirm } => {
            reset_baseline(&instrument, confirm).await
        }
    }
}

async fn list_baselines(instrument_filter: Option<String>) -> Result<()> {
    let config = Config::load()?;

    println!();

    // For v1, baselines are managed by the cloud
    // This would query the cloud API to list baselines
    // For now, show a placeholder

    let instruments: Vec<_> = if let Some(ref filter) = instrument_filter {
        config
            .instruments
            .iter()
            .filter(|i| i.id == *filter)
            .collect()
    } else {
        config.instruments.iter().collect()
    };

    if instruments.is_empty() {
        if instrument_filter.is_some() {
            println!("No instrument found matching filter");
        } else {
            println!("No instruments configured");
        }
        return Ok(());
    }

    for instrument in instruments {
        println!("Baselines for {}", instrument.id);
        println!("{}", "=".repeat(30 + instrument.id.len()));
        println!();

        // TODO: Query cloud for baselines
        // For now, show placeholder
        println!("[ACTIVE]   base_example  2026-01-15  {}", instrument.template);
        println!("           (baseline data would come from cloud)");
        println!();
    }

    Ok(())
}

async fn show_baseline(baseline_id: &str) -> Result<()> {
    println!();
    println!("Baseline Details: {}", baseline_id);
    println!("{}", "=".repeat(20 + baseline_id.len()));
    println!();

    // TODO: Query cloud for baseline details
    println!("(baseline details would come from cloud)");
    println!();
    println!("Fields that would be shown:");
    println!("  - Baseline ID");
    println!("  - Instrument ID");
    println!("  - Template name and hash");
    println!("  - Established date");
    println!("  - State (active/archived)");
    println!("  - Run metrics summary");
    println!("  - Target count");
    println!();

    Ok(())
}

async fn reset_baseline(instrument: &str, confirm: bool) -> Result<()> {
    let config = Config::load()?;

    // Verify instrument exists
    let inst = config
        .instruments
        .iter()
        .find(|i| i.id == instrument);

    if inst.is_none() {
        anyhow::bail!("Instrument '{}' not found in configuration", instrument);
    }

    println!();
    println!("WARNING: This will archive the current baseline for '{}'.", instrument);
    println!("A new SSC0 run will be required to establish a new baseline.");
    println!();

    if !confirm {
        print!("Proceed? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // TODO: Send reset request to cloud
    println!();
    println!("Baseline archived. Awaiting new SSC0 run.");
    println!();
    println!("(In production, this would send a request to the MD cloud)");
    println!();

    Ok(())
}
