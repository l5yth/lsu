# Copyright (c) 2026 l5yth
# SPDX-License-Identifier: Apache-2.0
#
# Standalone derivation for lsu.
# Can be called directly from a flake or from nixpkgs:
#   pkgs.callPackage ./packaging/nix/default.nix { }
{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "lsu";
  version = (lib.importTOML ../../Cargo.toml).package.version;

  src = lib.cleanSource ../..;

  cargoLock.lockFile = ../../Cargo.lock;

  meta = {
    description = "Terminal UI for systemd services and their journal";
    homepage    = "https://github.com/l5yth/lsu";
    license     = lib.licenses.asl20;
    maintainers = [ ];
    # lsu shells out to systemctl/journalctl; systemd is Linux-only.
    platforms   = lib.platforms.linux;
    mainProgram = "lsu";
  };
}
