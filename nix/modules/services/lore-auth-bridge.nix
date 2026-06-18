{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib) mkEnableOption mkIf mkOption types;
  cfg = config.services.lore-auth-bridge;
  tomlFormat = pkgs.formats.toml { };
  tomlLib = import ../../lib/toml.nix { inherit lib; };
  renderedSettings = tomlLib.filterNulls {
    bind = "${cfg.host}:${toString cfg.port}";
    public_base_url = cfg.publicBaseUrl;
    authentik = {
      issuer = cfg.authentik.issuer;
      client_id = cfg.authentik.clientId;
      client_secret = cfg.authentik.clientSecret;
      scopes = cfg.authentik.scopes;
      flow = cfg.authentik.flow;
    };
    jwt = {
      issuer = cfg.jwt.issuer;
      audience = cfg.jwt.audience;
      private_key_pem_file = cfg.jwt.privateKeyPemFile;
      public_jwks_json = cfg.jwt.publicJwksJson;
      public_jwks_json_file = cfg.jwt.publicJwksJsonFile;
    };
    policy = {
      default_resource_mode = cfg.policy.defaultResourceMode;
      registry_path = cfg.policy.registryPath;
    };
  };
  configFile = tomlFormat.generate "lore-auth-bridge.toml" renderedSettings;
in
{
  options.services.lore-auth-bridge = {
    enable = mkEnableOption "Lore Authentik UCS Auth bridge";

    package = mkOption {
      type = types.nullOr types.package;
      default = pkgs.lore-auth-bridge or null;
      defaultText = lib.literalExpression "pkgs.lore-auth-bridge";
      description = "Lore Auth bridge package.";
    };

    user = mkOption {
      type = types.str;
      default = "lore-auth-bridge";
      description = "User that runs lore-auth-bridge.";
    };

    group = mkOption {
      type = types.str;
      default = "lore-auth-bridge";
      description = "Group that runs lore-auth-bridge.";
    };

    stateDir = mkOption {
      type = types.str;
      default = "/var/lib/lore-auth-bridge";
      description = "Persistent state directory for bridge registry data.";
    };

    host = mkOption {
      type = types.str;
      default = "127.0.0.1";
      description = "Bind address.";
    };

    port = mkOption {
      type = types.port;
      default = 4180;
      description = "TCP port.";
    };

    publicBaseUrl = mkOption {
      type = types.str;
      description = "Public HTTPS base URL for callbacks and JWKS links.";
      example = "https://auth.lore.example";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Open the bridge TCP port.";
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = "Extra environment variables. Use LORE_AUTH_BRIDGE__... overrides for secrets if needed.";
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description = "Environment file loaded by systemd.";
    };

    authentik = {
      issuer = mkOption {
        type = types.str;
        description = "Authentik OAuth/OIDC issuer URL.";
      };
      clientId = mkOption {
        type = types.str;
        description = "Authentik OAuth client ID.";
      };
      clientSecret = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Optional Authentik OAuth client secret. Prefer environmentFile for secret deployments.";
      };
      scopes = mkOption {
        type = types.listOf types.str;
        default = [
          "openid"
          "profile"
          "email"
        ];
        description = "OAuth scopes requested from Authentik.";
      };
      flow = mkOption {
        type = types.enum [
          "device"
          "callback"
          "both"
        ];
        default = "device";
        description = "Interactive login flow exposed to Lore clients.";
      };
    };

    jwt = {
      issuer = mkOption {
        type = types.str;
        description = "Issuer claim for bridge-issued Lore JWTs.";
      };
      audience = mkOption {
        type = types.listOf types.str;
        description = "Audience/root-domain claims accepted by Lore clients and loreserver.";
      };
      privateKeyPemFile = mkOption {
        type = types.str;
        description = "Runtime path to the RS256 private key PEM used to sign Lore JWTs.";
        example = "/run/secrets/lore-auth-bridge/private-key.pem";
      };
      publicJwksJson = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Public JWKS JSON served at /.well-known/jwks.json.";
      };
      publicJwksJsonFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Runtime path to public JWKS JSON. Use this instead of publicJwksJson for generated files.";
      };
    };

    policy = {
      defaultResourceMode = mkOption {
        type = types.enum [
          "wildcard"
          "requested"
        ];
        default = "wildcard";
        description = "Whether repository token exchange grants urc-* or only requested resource IDs.";
      };
      registryPath = mkOption {
        type = types.str;
        default = "${cfg.stateDir}/resources.json";
        description = "Local registry file for ReBAC CreateResource/DeleteResource.";
      };
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.package != null;
        message = "services.lore-auth-bridge.package must be set when pkgs.lore-auth-bridge is not available.";
      }
      {
        assertion = cfg.jwt.publicJwksJson != null || cfg.jwt.publicJwksJsonFile != null;
        message = "Set services.lore-auth-bridge.jwt.publicJwksJson or publicJwksJsonFile.";
      }
    ];

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.stateDir;
    };

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ cfg.port ];

    systemd.tmpfiles.rules = [
      "d ${cfg.stateDir} 0750 ${cfg.user} ${cfg.group} - -"
    ];

    systemd.services.lore-auth-bridge = {
      description = "Lore Authentik UCS Auth bridge";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      environment = cfg.environment;

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/lore-auth-bridge --config ${configFile}";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.stateDir;
        StateDirectory = "lore-auth-bridge";
        EnvironmentFile = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ cfg.stateDir ];
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = false;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        SystemCallArchitectures = "native";
      };
    };
  };
}
