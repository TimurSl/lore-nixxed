# Use Lore with NixOS flakes

This repository exposes Nix packages and NixOS modules for the Lore CLI and a
plugin-enabled Lore Server. It also exposes an Authentik-backed UCS Auth bridge
for deployments that want `lore auth login` against an external identity
provider. For a full list of outputs and configuration options, see the
[Nix Flake reference](../reference/nix-flake.md).

## Install the CLI

```nix
{
  inputs.lore.url = "github:TimurSl/lore-nixxed";

  outputs = { self, nixpkgs, lore, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        lore.nixosModules.lore
        {
          nixpkgs.overlays = [ lore.overlays.default ];
          programs.lore.enable = true;
          programs.lore.settings.pager = "less -FRX";
        }
      ];
    };
  };
}
```

The module installs `lore`, shell completions, and `/etc/xdg/lore/cli.toml`.

## Run a persistent single-node server

```nix
{
  imports = [ inputs.lore.nixosModules.lore-server ];
  nixpkgs.overlays = [ inputs.lore.overlays.default ];

  services.lore-server.enable = true;
}
```

The server stores durable local state under `/var/lib/lore/store` and reads its
generated overlay from `/etc/lore-server/local.toml`.

## Add Authentik login through the UCS Auth bridge

```nix
{
  imports = [
    inputs.lore.nixosModules.lore-server
    inputs.lore.nixosModules.lore-auth-bridge
  ];
  nixpkgs.overlays = [ inputs.lore.overlays.default ];

  services.lore-server = {
    enable = true;
    settings = {
      environment.endpoint.auth_url = "ucs-auth://auth.lore.zenisoft.net.ua";

      server.auth = {
        jwt_issuer = "https://auth.lore.zenisoft.net.ua";
        jwt_audience = [ "lore.zenisoft.net.ua" ];
        jwk.endpoint = "https://auth.lore.zenisoft.net.ua/.well-known/jwks.json";
      };
    };
  };

  services.lore-auth-bridge = {
    enable = true;
    publicBaseUrl = "https://auth.lore.zenisoft.net.ua";
    authentik = {
      issuer = "https://sso.example/application/o/lore/";
      clientId = "lore-client";
      flow = "device";
    };
    jwt = {
      issuer = "https://auth.lore.zenisoft.net.ua";
      audience = [ "lore.zenisoft.net.ua" ];
      privateKeyPemFile = "/run/secrets/lore-auth-bridge/private.pem";
      publicJwksJsonFile = "/run/secrets/lore-auth-bridge/jwks.json";
    };
  };
}
```

The bridge signs Lore AuthN/AuthZ JWTs and serves the matching JWKS. Authentik
must have device-code flow enabled for the configured provider when
`authentik.flow = "device"`.

## Enable AWS storage and Consul topology

```nix
{
  services.lore-server = {
    enable = true;
    environmentFile = "/run/secrets/lore-server.env";

    settings = {
      immutable_store.mode = "aws";
      mutable_store.mode = "aws";
      lock_store.mode = "aws";
      topology.provider = "consul";

      plugins.aws = {
        immutable_store = {
          s3_bucket = "lore-fragments";
          dynamodb_fragments_table = "lore-fragments";
          dynamodb_metadata_table = "lore-fragment-metadata";
        };
        mutable_store.dynamodb_table = "lore-mutable";
        lock_store.dynamodb_table = "lore-locks";
      };

      plugins.consul = {
        service_name = "lore-server";
        client_config.address = "http://consul.service.consul:8500";
      };
    };
  };
}
```

Put secret values in the environment file, for example
`LORE__SERVER__HTTP__PRESIGNED_URL_HMAC_KEY=...`. AWS credentials are resolved by
the AWS SDK environment, profile, or role provider chain.

## Manage a repository preset

```nix
{
  programs.lore = {
    enable = true;
    repositories.demo = {
      path = "/srv/lore/demo";
      owner = "alice";
      group = "users";
      config = {
        remote_url = "lore://127.0.0.1:41337/demo";
        identity = "alice@example.com";
        store.max_size = 1073741824;
      };
    };
  };
}
```

Declared repository presets overwrite `<path>/.lore/config.toml` on rebuild.
