# Copyright (c) 2026 l5yth
# SPDX-License-Identifier: Apache-2.0
{
  description = "Terminal UI for systemd services and their journal";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs   = nixpkgs.legacyPackages.${system};
    in {
      packages.${system}.default =
        pkgs.callPackage ./packaging/nix/default.nix { };

      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ self.packages.${system}.default ];
        packages   = [ pkgs.cargo pkgs.rustfmt pkgs.clippy ];
      };
    };
}
