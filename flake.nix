{
  description = "ReadingSteiner - web/data change detection with Telegram push";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f system);
      # NixOS 集成测试需要 KVM；无 KVM 环境（普通 CI runner）自动跳过
      hasKvm = builtins.pathExists "/dev/kvm";
    in
    {
      nixosModules.default = import ./nixos/module.nix;
      nixosModules.reading-steiner = import ./nixos/module.nix;

      overlays.default = final: _prev: {
        reading-steiner = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      checks = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;

          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            pname = "reading-steiner";
            version = "0.1.0";
            buildInputs = [ ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.darwin.apple_sdk.frameworks.Security ];
            nativeBuildInputs = [ pkgs.pkg-config ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          package = craneLib.buildPackage (commonArgs // { cargoArtifacts = cargoArtifacts; });
        in
        {
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
          # NixOS 集成测试（需要 KVM，无 KVM 自动跳过）
          nixos-test = (nixpkgs.lib.optionalAttrs hasKvm {
            ${system} = import ./nixos/tests/reading-steiner.nix {
              inherit pkgs;
              reading-steiner = self;
            };
          }).${system} or (pkgs.runCommand "reading-steiner-nixos-test-skipped" { } ''
            echo "skipped: no /dev/kvm" > $out
          '');
        });

      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;

          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            pname = "reading-steiner";
            version = "0.1.0";
            buildInputs = [ ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.darwin.apple_sdk.frameworks.Security ];
            nativeBuildInputs = [ pkgs.pkg-config ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          package = craneLib.buildPackage (commonArgs // {
            cargoArtifacts = cargoArtifacts;
            meta.mainProgram = "reading-steiner";
          });

          # Web 控制台前端（pnpm 构建），产物随包分发
          web = pkgs.stdenv.mkDerivation {
            pname = "reading-steiner-web";
            version = "0.1.0";
            src = ./web;

            nativeBuildInputs = [ pkgs.nodejs pkgs.pnpm_11.configHook ];
            pnpmDeps = pkgs.fetchPnpmDeps {
              pname = "reading-steiner-web";
              version = "0.1.0";
              src = ./web;
              fetcherVersion = 4;
              hash = "sha256-YmmIokyDuMT8LfNJrY/VZ2E2LTykPJHHPwOCsEwc+mw=";
            };

            buildPhase = "pnpm build";
            installPhase = "mkdir $out && cp -r dist/* $out";
          };
        in
        {
          default = package;
          reading-steiner = package;
          inherit web;
        });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
        in
        {
          default = craneLib.devShell {
            checks = self.checks.${system};
            packages = [ pkgs.cargo-nextest pkgs.rust-analyzer ];
          };
        });

      # 模块求值冒烟测试：接入模块并确认可求值（无需 KVM / 无构建）。
      # fileSystems/boot loader 为求值占位，不做实际构建。
      nixosConfigurations.smoke-test = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          self.nixosModules.default
          ({ ... }: {
            services.reading-steiner.enable = true;
            services.reading-steiner.package = self.packages.x86_64-linux.default;
            fileSystems."/" = {
              device = "/dev/disk/by-label/nixos";
              fsType = "ext4";
            };
            boot.loader.grub.devices = [ "/dev/vda" ];
          })
        ];
      };
    };
}
