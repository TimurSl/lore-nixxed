{
  lib,
  rustPlatform,
  protobuf,
  pkg-config,
  src,
}:

rustPlatform.buildRustPackage {
  pname = "lore-workspace";
  version = "0.8.4-nightly";

  src =
    let
      root = toString src;
    in
    lib.cleanSourceWith {
      src = lib.cleanSource src;
      filter =
        path: type:
        let
          rel = lib.removePrefix "${root}/" (toString path);
        in
        !(lib.hasPrefix "nix/" rel)
        && rel != "flake.nix"
        && rel != "flake.lock"
        && !(lib.hasPrefix "result" rel);
    };

  cargoLock = {
    lockFile = "${src}/Cargo.lock";
  };

  nativeBuildInputs = [
    pkg-config
    protobuf
  ];

  RUSTFLAGS = lib.concatStringsSep " " [
    "--cfg tokio_unstable"
    "--cfg uuid_unstable"
    "-C force-unwind-tables=yes"
    "-C force-frame-pointers=yes"
  ];

  # The upstream workspace has a large integration suite that expects live
  # services. Package checks are handled by the flake module checks instead.
  doCheck = false;

  meta = {
    description = "Lore version-control client and server";
    license = lib.licenses.mit;
    mainProgram = "lore";
    platforms = lib.platforms.linux;
  };
}
