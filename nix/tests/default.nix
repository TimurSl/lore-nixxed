{
  pkgs,
  lib,
  self,
  system,
}:

let
  evalSystem =
    module:
    lib.nixosSystem {
      inherit system;
      modules = [
        self.nixosModules.default
        ({ pkgs, ... }: {
          nixpkgs.overlays = [ self.overlays.default ];
          system.stateVersion = "24.11";
        })
        module
      ];
    };

  minimalServer = evalSystem {
    services.lore-server.enable = true;
  };

  firewallServer = evalSystem {
    services.lore-server = {
      enable = true;
      openFirewall = true;
      settings.server = {
        quic = {
          enabled = true;
          port = 41000;
          verify_client_certs = false;
        };
        quic_internal = {
          enabled = true;
          port = 41001;
        };
        grpc = {
          enabled = true;
          port = 41002;
        };
        http = {
          enabled = true;
          port = 41003;
        };
        replication = {
          enabled = true;
          port = 41004;
        };
      };
    };
  };

  pluginServer = evalSystem {
    services.lore-server = {
      enable = true;
      settings = {
        immutable_store.mode = "aws";
        mutable_store.mode = "aws";
        lock_store.mode = "aws";
        topology.provider = "consul";
        plugins.aws = {
          immutable_store = {
            s3_bucket = "lore-fragments";
            dynamodb_fragments_table = "lore-fragments";
            dynamodb_metadata_table = "lore-metadata";
          };
          mutable_store.dynamodb_table = "lore-mutable";
          lock_store.dynamodb_table = "lore-locks";
        };
        plugins.consul = {
          service_name = "lore-server";
          client_config.address = "http://127.0.0.1:8500";
          poll_interval_secs = 30;
        };
      };
    };
  };

  clientConfig = evalSystem {
    programs.lore = {
      enable = true;
      enableCompletions = false;
      settings.pager = "less -FRX";
      repositories.demo = {
        path = "/srv/lore/demo";
        owner = "root";
        group = "root";
        config = {
          remote_url = "lore://127.0.0.1:41337/demo";
          identity = "demo@example.com";
          store.max_size = 1048576;
        };
      };
    };
  };

  assertContains = haystack: needle:
    assert lib.assertMsg (lib.hasInfix needle haystack) "expected '${needle}' in:\n${haystack}";
    true;

  minimalToml = builtins.readFile minimalServer.config.environment.etc."lore-server/local.toml".source;
  pluginToml = builtins.readFile pluginServer.config.environment.etc."lore-server/local.toml".source;
  clientActivation = clientConfig.config.system.activationScripts.loreRepositories.text;
in
{
  module-generated-persistent-server-toml = pkgs.runCommand "lore-server-module-generated-toml" { } ''
    ${lib.optionalString (assertContains minimalToml ''path = "/var/lib/lore/store/immutable"'') ""}
    ${lib.optionalString (assertContains minimalToml ''path = "/var/lib/lore/store/mutable"'') ""}
    touch "$out"
  '';

  module-open-firewall-ports = pkgs.runCommand "lore-server-module-firewall" { } ''
    ${lib.optionalString (builtins.elem 41002 firewallServer.config.networking.firewall.allowedTCPPorts) ""}
    ${lib.optionalString (builtins.elem 41003 firewallServer.config.networking.firewall.allowedTCPPorts) ""}
    ${lib.optionalString (builtins.elem 41004 firewallServer.config.networking.firewall.allowedTCPPorts) ""}
    ${lib.optionalString (builtins.elem 41000 firewallServer.config.networking.firewall.allowedUDPPorts) ""}
    ${lib.optionalString (builtins.elem 41001 firewallServer.config.networking.firewall.allowedUDPPorts) ""}
    touch "$out"
  '';

  module-plugin-toml-shape = pkgs.runCommand "lore-server-module-plugin-shape" { } ''
    ${lib.optionalString (assertContains pluginToml ''mode = "aws"'') ""}
    ${lib.optionalString (assertContains pluginToml ''s3_bucket = "lore-fragments"'') ""}
    ${lib.optionalString (assertContains pluginToml ''dynamodb_fragments_table = "lore-fragments"'') ""}
    ${lib.optionalString (assertContains pluginToml ''dynamodb_table = "lore-mutable"'') ""}
    ${lib.optionalString (assertContains pluginToml ''dynamodb_table = "lore-locks"'') ""}
    ${lib.optionalString (assertContains pluginToml ''provider = "consul"'') ""}
    ${lib.optionalString (assertContains pluginToml ''service_name = "lore-server"'') ""}
    touch "$out"
  '';

  module-managed-repository-activation = pkgs.runCommand "lore-client-module-repository-activation" { } ''
    ${lib.optionalString (assertContains clientActivation "/srv/lore/demo/.lore") ""}
    ${lib.optionalString (assertContains clientActivation "config.toml") ""}
    touch "$out"
  '';
}
