# Nix Flake Reference

The Lore project provides a first-class Nix integration via a Flake. This allows you to install the Lore CLI, run the Lore Server, and manage your Lore configuration declaratively on NixOS.

## Overview

The Lore flake is compatible with `x86_64-linux` and `aarch64-linux`. It follows the standard flake output structure.

- **Packages**: `lore` (CLI) and `loreserver` (Server with plugins).
- **NixOS Modules**: `programs.lore` and `services.lore-server`.
- **Overlays**: `default` overlay adding `lore` and `loreserver` to `pkgs`.
- **Checks**: Integration tests for both CLI and Server.

## Flake Outputs

### Packages (`packages.<system>`)

- **`lore`**: The Lore CLI package. Includes shell completions for Bash and Zsh.
- **`loreserver`**: The Lore Server package. This version is built with plugin support (AWS, Consul, etc.).
- **`default`**: Points to `lore`.

### NixOS Modules (`nixosModules`)

- **`lore`**: Configures the Lore CLI and manages repository-level configurations.
- **`lore-server`**: Configures and runs the `loreserver` as a systemd service.
- **`default`**: Imports both `lore` and `lore-server` modules.

### Overlays (`overlays`)

- **`default`**: Adds `lore` and `loreserver` to your `nixpkgs` set.

## NixOS Module: `programs.lore`

This module installs the Lore CLI and optionally manages `.lore/config.toml` files for specific repositories.

### Key Options

- **`programs.lore.enable`**: (bool) Whether to install the Lore CLI.
- **`programs.lore.package`**: (package) The Lore package to use (defaults to `pkgs.lore`).
- **`programs.lore.settings`**: (attrs) Global settings written to `/etc/xdg/lore/cli.toml`.
  - `pager`: (string) The pager command to use.
- **`programs.lore.repositories`**: (attrs) Manage `.lore/config.toml` for repositories.
  - `path`: (path) The working-tree path of the repository.
  - `owner`/`group`: (string) Ownership of the `.lore` directory.
  - `config`: (attrs) TOML content for the repository config.

## NixOS Module: `services.lore-server`

This module runs the Lore Server as a hardened systemd service.

### Key Options

- **`services.lore-server.enable`**: (bool) Whether to enable the Lore Server.
- **`services.lore-server.package`**: (package) The Lore Server package to use (defaults to `pkgs.loreserver`).
- **`services.lore-server.settings`**: (attrs) TOML settings rendered to `/etc/lore-server/local.toml`. This follows the structure defined in [Lore Server configuration reference](lore-server-config.md).
- **`services.lore-server.stateDir`**: (string) Persistent state directory (default: `/var/lib/lore`).
- **`services.lore-server.configDir`**: (string) Runtime config directory (default: `/etc/lore-server`).
- **`services.lore-server.environmentFile`**: (path) Path to a file containing environment variables (e.g., secrets).
- **`services.lore-server.openFirewall`**: (bool) Automatically open ports in the NixOS firewall based on `settings`.

## Usage Examples

### Using the Overlay in a Flake

```nix
{
  inputs.lore.url = "github:your-org/lore";

  outputs = { self, nixpkgs, lore, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        lore.nixosModules.default
        {
          nixpkgs.overlays = [ lore.overlays.default ];
          
          programs.lore.enable = true;
          services.lore-server = {
            enable = true;
            settings.server.http.port = 8080;
          };
        }
      ];
    };
  };
}
```

### Running the CLI without installation

```bash
# Run the default package (lore)
nix run github:your-org/lore -- --help

# Run the server
nix run github:your-org/lore#loreserver -- --help
```

### Running Tests

```bash
nix flake check github:your-org/lore
```
