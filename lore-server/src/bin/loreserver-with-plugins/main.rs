// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
// Copyright Epic Games, Inc. All Rights Reserved.

use anyhow::Result;
use lore_server::plugins;
use lore_server::server::server_main;
use lore_server::server_config::ServerConfig;

fn main() -> Result<()> {
    let mut config = ServerConfig::default();
    plugins::register_all_plugins(&mut config.plugin_registry);
    server_main(config)
}
