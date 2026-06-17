{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib) mkEnableOption mkIf mkOption types;
  cfg = config.services.lore-server;
  typesModule = import ../types.nix { inherit config lib pkgs; };
  tomlFormat = pkgs.formats.toml { };
  tomlLib = import ../../lib/toml.nix { inherit lib; };
  renderedSettings = tomlLib.filterNulls cfg.settings;
  configFile = tomlFormat.generate "lore-server-local.toml" renderedSettings;
  configDirEtcName = lib.removePrefix "/etc/" cfg.configDir;

  enabledTcpPorts =
    lib.optional (cfg.settings.server.grpc != null && cfg.settings.server.grpc.enabled) cfg.settings.server.grpc.port
    ++ lib.optional (cfg.settings.server.http != null && cfg.settings.server.http.enabled) cfg.settings.server.http.port
    ++ lib.optional (cfg.settings.server.replication != null && cfg.settings.server.replication.enabled) cfg.settings.server.replication.port;
  enabledUdpPorts =
    lib.optional (cfg.settings.server.quic != null && cfg.settings.server.quic.enabled) cfg.settings.server.quic.port
    ++ lib.optional (cfg.settings.server.quic_internal != null && cfg.settings.server.quic_internal.enabled) cfg.settings.server.quic_internal.port;
in
{
  options.services.lore-server = {
    enable = mkEnableOption "Lore Server";

    package = mkOption {
      type = types.nullOr types.package;
      default = pkgs.loreserver or null;
      defaultText = lib.literalExpression "pkgs.loreserver";
      description = "Plugin-enabled Lore Server package.";
    };

    user = mkOption {
      type = types.str;
      default = "lore";
      description = "User that runs loreserver.";
    };

    group = mkOption {
      type = types.str;
      default = "lore";
      description = "Group that runs loreserver.";
    };

    stateDir = mkOption {
      type = types.str;
      default = "/var/lib/lore";
      description = "Persistent state directory.";
    };

    configDir = mkOption {
      type = types.str;
      default = "/etc/lore-server";
      description = "Runtime config directory containing local.toml.";
    };

    logDir = mkOption {
      type = types.str;
      default = "/var/log/lore";
      description = "Log directory reserved for deployments that log to files.";
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = "Environment variables for loreserver. Use LORE__... variables for secret config overrides.";
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description = "Environment file loaded by systemd. Use this for presigned URL HMAC keys and other secrets.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Open enabled Lore TCP and UDP endpoint ports.";
    };

    settings = mkOption {
      type = typesModule.serverSettingsType;
      default = { };
      description = "Lore Server TOML settings rendered to local.toml.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.package != null;
        message = "services.lore-server.package must be set when pkgs.loreserver is not available.";
      }
      {
        assertion =
          !(renderedSettings.server.http or { }) ? presigned_url_hmac_key;
        message = "Do not put server.http.presigned_url_hmac_key in services.lore-server.settings; set LORE__SERVER__HTTP__PRESIGNED_URL_HMAC_KEY through environmentFile or environment.";
      }
      {
        assertion = lib.hasPrefix "/etc/" cfg.configDir;
        message = "services.lore-server.configDir must live under /etc because the module manages local.toml through environment.etc.";
      }
    ];

    services.lore-server.settings = {
      immutable_store.local.path = lib.mkDefault "${cfg.stateDir}/store/immutable";
      mutable_store.local.path = lib.mkDefault "${cfg.stateDir}/store/mutable";
    };

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.stateDir;
    };

    environment.etc."${configDirEtcName}/local.toml".source = configFile;

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall enabledTcpPorts;
    networking.firewall.allowedUDPPorts = mkIf cfg.openFirewall enabledUdpPorts;

    systemd.tmpfiles.rules = [
      "d ${cfg.stateDir} 0750 ${cfg.user} ${cfg.group} - -"
      "d ${cfg.stateDir}/store 0750 ${cfg.user} ${cfg.group} - -"
      "d ${cfg.stateDir}/store/immutable 0750 ${cfg.user} ${cfg.group} - -"
      "d ${cfg.stateDir}/store/mutable 0750 ${cfg.user} ${cfg.group} - -"
      "d ${cfg.logDir} 0750 ${cfg.user} ${cfg.group} - -"
    ];

    systemd.services.lore-server = {
      description = "Lore Server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      environment = {
        LORE_CONFIG_PATH = cfg.configDir;
        LORE_ENV = "local";
      } // cfg.environment;

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/loreserver";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.stateDir;
        StateDirectory = "lore";
        LogsDirectory = "lore";
        EnvironmentFile = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [
          cfg.stateDir
          cfg.logDir
        ];
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
