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
}
