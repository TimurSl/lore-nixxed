# Nix Flake Reference

The Lore project provides a first-class Nix integration via a Flake. This allows you to install the Lore CLI, run the Lore Server, and manage your Lore configuration declaratively on NixOS.

## Overview

The Lore flake is compatible with `x86_64-linux` and `aarch64-linux`. It follows the standard flake output structure.

- **Packages**: `lore` (CLI), `loreserver` (Server with plugins), and `lore-auth-bridge` (Authentik-backed UCS Auth bridge).
- **NixOS Modules**: `programs.lore`, `services.lore-server`, and `services.lore-auth-bridge`.
- **Overlays**: `default` overlay adding `lore`, `loreserver`, and `lore-auth-bridge` to `pkgs`.
- **Checks**: Integration tests for both CLI and Server.

## Flake Outputs

### Packages (`packages.<system>`)

- **`lore`**: The Lore CLI package. Includes shell completions for Bash and Zsh.
- **`loreserver`**: The Lore Server package. This version is built with plugin support (AWS, Consul, etc.).
- **`lore-auth-bridge`**: UCS Auth-compatible gRPC bridge that delegates user login to Authentik and signs Lore JWTs.
- **`default`**: Points to `lore`.

### NixOS Modules (`nixosModules`)

- **`lore`**: Configures the Lore CLI and manages repository-level configurations.
- **`lore-server`**: Configures and runs the `loreserver` as a systemd service.
- **`lore-auth-bridge`**: Configures and runs the Authentik UCS Auth bridge as a systemd service.
- **`default`**: Imports `lore`, `lore-server`, and `lore-auth-bridge` modules.

### Overlays (`overlays`)

- **`default`**: Adds `lore`, `loreserver`, and `lore-auth-bridge` to your `nixpkgs` set.

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

## NixOS Module: `services.lore-auth-bridge`

This module runs the Authentik-backed UCS Auth bridge as a hardened systemd service.

### Key Options

- **`services.lore-auth-bridge.enable`**: (bool) Whether to enable the bridge.
- **`services.lore-auth-bridge.package`**: (package) The bridge package to use (defaults to `pkgs.lore-auth-bridge`).
- **`services.lore-auth-bridge.publicBaseUrl`**: Public HTTPS URL for callbacks and JWKS, for example `https://auth.lore.example`.
- **`services.lore-auth-bridge.authentik.issuer`**: Authentik OAuth/OIDC issuer URL.
- **`services.lore-auth-bridge.authentik.clientId`**: Authentik OAuth client ID.
- **`services.lore-auth-bridge.authentik.flow`**: `device`, `callback`, or `both`; defaults to `device`.
- **`services.lore-auth-bridge.jwt.issuer`**: JWT issuer claim for bridge-issued Lore tokens.
- **`services.lore-auth-bridge.jwt.audience`**: JWT audiences/root domains accepted by Lore clients and `loreserver`.
- **`services.lore-auth-bridge.jwt.privateKeyPemFile`**: Runtime path to the RS256 private key PEM.
- **`services.lore-auth-bridge.jwt.publicJwksJson`** or **`publicJwksJsonFile`**: Public JWKS served by the bridge.
- **`services.lore-auth-bridge.policy.defaultResourceMode`**: `wildcard` grants `urc-*`; `requested` grants only requested resource IDs.

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
            settings.environment.endpoint.auth_url = "ucs-auth://auth.lore.example";
            settings.server.http.port = 8080;
            settings.server.auth = {
              jwt_issuer = "https://auth.lore.example";
              jwt_audience = [ "lore.example" ];
              jwk.endpoint = "https://auth.lore.example/.well-known/jwks.json";
            };
          };

          services.lore-auth-bridge = {
            enable = true;
            publicBaseUrl = "https://auth.lore.example";
            authentik = {
              issuer = "https://sso.example/application/o/lore/";
              clientId = "lore-client";
            };
            jwt = {
              issuer = "https://auth.lore.example";
              audience = [ "lore.example" ];
              privateKeyPemFile = "/run/secrets/lore-auth-bridge/private.pem";
              publicJwksJsonFile = "/run/secrets/lore-auth-bridge/jwks.json";
            };
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
