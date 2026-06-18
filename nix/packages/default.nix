{
  pkgs,
  lib,
  src,
}:

let
  common = pkgs.callPackage ./rust.nix { inherit src; };
in
{
  lore = common.overrideAttrs (_old: {
    pname = "lore";
    cargoBuildFlags = [ "--bin=lore" ];
    cargoTestFlags = [ "--bin=lore" ];
  });

  loreserver = common.overrideAttrs (_old: {
    pname = "loreserver";
    cargoBuildFlags = [ "--bin=loreserver" ];
    cargoTestFlags = [ "--bin=loreserver" ];
  });

  lore-auth-bridge = common.overrideAttrs (_old: {
    pname = "lore-auth-bridge";
    cargoBuildFlags = [ "--bin=lore-auth-bridge" ];
    cargoTestFlags = [ "--package=lore-auth-bridge" ];
    meta = common.meta // {
      description = "Authentik-backed UCS Auth bridge for Lore";
      mainProgram = "lore-auth-bridge";
    };
  });
}
