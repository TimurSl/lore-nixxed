// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use clap::Parser;
use lore_auth_bridge::AppState;
use lore_auth_bridge::BridgeConfig;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let env_config = std::env::var("LORE_AUTH_BRIDGE_CONFIG").ok();
    let config_path = args.config.as_deref().or(env_config.as_deref());
    let config = BridgeConfig::load(config_path)?;
    AppState::new(config)?.serve().await?;
    Ok(())
}
