{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib) mkOption types;

  nullable = type: types.nullOr type;
  attrs = types.attrsOf types.anything;

  certificateType = types.submodule {
    options = {
      cert_file = mkOption {
        type = types.path;
        description = "PEM-encoded certificate file.";
      };
      pkey_file = mkOption {
        type = types.path;
        description = "PEM-encoded private key file. The file path is referenced, not embedded.";
      };
      cert_chain = mkOption {
        type = nullable types.path;
        default = null;
        description = "Optional PEM certificate chain used for mTLS verification.";
      };
    };
  };

  quicEndpointType = types.submodule {
    options = {
      enabled = mkOption {
        type = types.bool;
        default = false;
        description = "Whether to start this QUIC endpoint.";
      };
      host = mkOption {
        type = types.str;
        default = "0.0.0.0";
        description = "Bind address.";
      };
      port = mkOption {
        type = types.port;
        default = 41337;
        description = "UDP port.";
      };
      verify_client_certs = mkOption {
        type = types.bool;
        default = true;
        description = "Whether this endpoint requires client certificates.";
      };
      certificate = mkOption {
        type = nullable certificateType;
        default = null;
        description = "Optional TLS certificate files.";
      };
      idle_timeout = mkOption { type = nullable types.int; default = null; };
      keep_alive = mkOption { type = nullable types.int; default = null; };
      max_bidi_streams = mkOption { type = nullable types.int; default = null; };
      num_listeners = mkOption { type = nullable types.int; default = null; };
      transport_bits_per_second = mkOption { type = nullable types.int; default = null; };
      transport_rtt = mkOption { type = nullable types.int; default = null; };
      handler_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      connection_message_limit = mkOption { type = nullable types.int; default = null; };
    };
  };

  grpcEndpointType = types.submodule {
    options = {
      enabled = mkOption {
        type = types.bool;
        default = true;
        description = "Whether to start this gRPC endpoint.";
      };
      host = mkOption {
        type = types.str;
        default = "0.0.0.0";
        description = "Bind address.";
      };
      port = mkOption {
        type = types.port;
        default = 41337;
        description = "TCP port.";
      };
      certificate = mkOption {
        type = nullable certificateType;
        default = null;
        description = "Optional TLS certificate files.";
      };
      request_handler_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      http2_keepalive_interval_seconds = mkOption { type = nullable types.int; default = null; };
      http2_keepalive_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      verify_client_certs = mkOption {
        type = types.bool;
        default = true;
        description = "Whether this endpoint requires client certificates.";
      };
    };
  };

  httpEndpointType = types.submodule {
    options = {
      enabled = mkOption { type = types.bool; default = true; };
      host = mkOption { type = types.str; default = "0.0.0.0"; };
      port = mkOption { type = types.port; default = 41339; };
      certificate = mkOption { type = nullable certificateType; default = null; };
      max_file_size = mkOption { type = nullable types.int; default = null; };
      request_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      request_body_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      available_interval_seconds = mkOption { type = nullable types.int; default = null; };
      available_timeout_seconds = mkOption { type = nullable types.int; default = null; };
      store_health_check = mkOption { type = nullable types.bool; default = null; };
      presigned_url_min_ttl_seconds = mkOption { type = nullable types.int; default = null; };
      presigned_url_default_ttl_seconds = mkOption { type = nullable types.int; default = null; };
      presigned_url_max_ttl_seconds = mkOption { type = nullable types.int; default = null; };
    };
  };

  localImmutableStoreType = types.submodule {
    options = {
      path = mkOption { type = types.str; default = "/var/lib/lore/store/immutable"; };
      flush_delay_seconds = mkOption { type = types.int; default = 10; };
      max_capacity = mkOption { type = nullable types.int; default = null; };
      max_size = mkOption { type = nullable types.int; default = null; };
      eviction_delay = mkOption { type = nullable types.int; default = null; };
      compaction_delay = mkOption { type = nullable types.int; default = null; };
      target_capacity_percentage = mkOption { type = nullable types.int; default = null; };
      target_size_percentage = mkOption { type = nullable types.int; default = null; };
      compaction_parallel_groups = mkOption { type = nullable types.int; default = null; };
    };
  };

  localMutableStoreType = types.submodule {
    options = {
      path = mkOption { type = types.str; default = "/var/lib/lore/store/mutable"; };
      flush_delay_seconds = mkOption { type = types.int; default = 10; };
    };
  };

  remoteStoreType = types.submodule {
    options = {
      remote_url = mkOption { type = types.str; description = "Remote store URL."; };
      auth_url = mkOption { type = nullable types.str; default = null; };
    };
  };

  replicatedStoreType = types.submodule {
    freeformType = attrs;
    options = {
      remote_url = mkOption { type = types.str; description = "Replication remote URL."; };
      certs = mkOption { type = nullable certificateType; default = null; };
    };
  };

  compositeSubStoreType = types.submodule {
    freeformType = attrs;
    options = {
      mode = mkOption { type = types.str; default = "local"; };
      local = mkOption { type = nullable localImmutableStoreType; default = null; };
      remote = mkOption { type = nullable remoteStoreType; default = null; };
      replicated = mkOption { type = nullable replicatedStoreType; default = null; };
      replication_mode = mkOption { type = nullable (types.enum [ "read" "write" "read_write" ]); default = null; };
    };
  };

  immutableStoreType = types.submodule {
    options = {
      mode = mkOption {
        type = types.str;
        default = "local";
        description = "Immutable store mode: local, composite, replicated, remote, or plugin name.";
      };
      local = mkOption { type = nullable localImmutableStoreType; default = { }; };
      remote = mkOption { type = nullable remoteStoreType; default = null; };
      replicated = mkOption { type = nullable replicatedStoreType; default = null; };
      composite = mkOption {
        type = nullable (types.submodule {
          freeformType = attrs;
          options = {
            local = mkOption { type = compositeSubStoreType; default = { mode = "local"; local = { }; }; };
            durable = mkOption { type = nullable compositeSubStoreType; default = null; };
            replica = mkOption { type = nullable (types.listOf compositeSubStoreType); default = null; };
            should_cache_query_results = mkOption { type = nullable types.bool; default = null; };
          };
        });
        default = null;
      };
    };
  };

  mutableStoreType = types.submodule {
    options = {
      mode = mkOption { type = types.str; default = "local"; };
      local = mkOption { type = nullable localMutableStoreType; default = { }; };
      remote = mkOption { type = nullable remoteStoreType; default = null; };
    };
  };

  lockStoreType = types.submodule {
    options.mode = mkOption {
      type = types.str;
      default = "local";
      description = "Lock store mode: local or plugin name.";
    };
  };

  telemetryType = types.submodule {
    freeformType = attrs;
    options = {
      logger = mkOption { type = attrs; default = { }; };
      metrics = mkOption { type = attrs; default = { }; };
      traces = mkOption { type = attrs; default = { }; };
    };
  };

  topologyType = types.submodule {
    options = {
      provider = mkOption {
        type = types.enum [ "none" "fixed" "rotating_id_fixed" "composite" "consul" ];
        default = "none";
        description = "Topology provider.";
      };
      fixed.peers = mkOption {
        type = types.listOf attrs;
        default = [ ];
        description = "Fixed peer entries with address, port, and locality.";
      };
      rotating_id_fixed = mkOption { type = attrs; default = { }; };
      composite = mkOption { type = attrs; default = { }; };
    };
  };

  awsPluginType = types.submodule {
    options = {
      http = mkOption { type = attrs; default = { }; };
      immutable_store = mkOption { type = attrs; default = { }; };
      mutable_store = mkOption { type = attrs; default = { }; };
      lock_store = mkOption { type = attrs; default = { }; };
    };
  };

  consulPluginType = types.submodule {
    freeformType = attrs;
    options = {
      client_config = mkOption { type = nullable attrs; default = null; };
      service_name = mkOption { type = nullable types.str; default = null; };
      ignore_address = mkOption { type = nullable types.str; default = null; };
      poll_interval_secs = mkOption { type = nullable types.int; default = null; };
    };
  };

  pluginsType = types.submodule {
    freeformType = attrs;
    options = {
      aws = mkOption { type = awsPluginType; default = { }; };
      consul = mkOption { type = consulPluginType; default = { }; };
    };
  };

  serverSettingsType = types.submodule {
    freeformType = attrs;
    options = {
      server = mkOption {
        type = types.submodule {
          freeformType = attrs;
          options = {
            connection_close_timeout_seconds = mkOption { type = nullable types.int; default = null; };
            runtime_shutdown_timeout_seconds = mkOption { type = nullable types.int; default = null; };
            quic = mkOption { type = nullable quicEndpointType; default = { enabled = true; verify_client_certs = false; }; };
            quic_internal = mkOption { type = nullable quicEndpointType; default = { port = 41340; }; };
            grpc = mkOption { type = nullable grpcEndpointType; default = { }; };
            replication = mkOption { type = nullable grpcEndpointType; default = { enabled = false; port = 41340; }; };
            http = mkOption { type = nullable httpEndpointType; default = { }; };
            grpc_public_services = mkOption { type = attrs; default = { }; };
            auth = mkOption { type = nullable attrs; default = null; };
            user_agent = mkOption { type = attrs; default = { }; };
          };
        };
        default = { };
      };
      immutable_store = mkOption { type = immutableStoreType; default = { }; };
      mutable_store = mkOption { type = mutableStoreType; default = { }; };
      lock_store = mkOption { type = nullable lockStoreType; default = { }; };
      telemetry = mkOption { type = nullable telemetryType; default = { }; };
      notification = mkOption { type = attrs; default = { mode = "local"; }; };
      topology = mkOption { type = nullable topologyType; default = { }; };
      plugins = mkOption { type = pluginsType; default = { }; };
      hooks = mkOption { type = attrs; default = { }; };
      feature = mkOption { type = attrs; default = { }; };
      tokio = mkOption { type = attrs; default = { }; };
    };
  };
in
{
  inherit
    certificateType
    serverSettingsType
    ;
}
