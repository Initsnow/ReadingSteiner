{
  description = "ReadingSteiner - web/data change detection with Telegram push";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          pname = "reading-steiner";
          version = "0.1.0";
          buildInputs = [ ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.darwin.apple_sdk.frameworks.Security ];
          nativeBuildInputs = [ pkgs.pkg-config ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        package = craneLib.buildPackage (commonArgs // { cargoArtifacts = cargoArtifacts; });
      in
      {
        packages.default = package;
        packages.reading-steiner = package;

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = [ pkgs.cargo-nextest pkgs.rust-analyzer ];
        };

        checks = {
          inherit package;
          fmt = craneLib.cargoFmt {
            src = craneLib.cleanCargoSource ./.;
          };
          clippy = craneLib.cargoClippy {
            src = craneLib.cleanCargoSource ./.;
            cargoArtifacts = cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          };
          unit = craneLib.cargoTest {
            src = craneLib.cleanCargoSource ./.;
            cargoArtifacts = cargoArtifacts;
          };
        };
      }) // {
      nixosModules.default = import ./nixos/module.nix;
      nixosModules.reading-steiner = import ./nixos/module.nix;
    };
}
