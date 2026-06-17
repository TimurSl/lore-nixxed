{
  description = "Lore CLI and Lore Server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    flake-utils.lib.eachSystem systems (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        packages = import ./nix/packages { inherit pkgs lib; src = self; };
      in
      {
        packages = packages // {
          default = packages.lore;
        };

        checks = import ./nix/tests {
          inherit pkgs lib self system;
        };
      }
    )
    // {
      nixosModules = {
        lore = import ./nix/modules/programs/lore.nix;
        lore-server = import ./nix/modules/services/lore-server.nix;
        default = {
          imports = [
            self.nixosModules.lore
            self.nixosModules.lore-server
          ];
        };
      };

      overlays.default = final: prev: {
        lore = self.packages.${final.system}.lore;
        loreserver = self.packages.${final.system}.loreserver;
      };
    };
}
