{
  description = "A full Rust flake";

  inputs = {
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };

    treefmt-nix.url = "github:numtide/treefmt-nix";
    git-hooks-nix.url = "github:cachix/git-hooks.nix";

    systems.url = "github:nix-systems/default";

    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-parts,
      crane,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import inputs.systems;

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        {
          config,
          system,
          pkgs,
          lib,
          ...
        }:
        let
          rust-bin = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain rust-bin;

          unfilteredRoot = ./.; # The original, unfiltered source
          src = lib.fileset.toSource {
            root = unfilteredRoot;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources unfilteredRoot)
              (lib.fileset.fileFilter (file: file.hasExt "pest") unfilteredRoot)
            ];
          };

          commonArgs = {
            inherit src;
            strictDeps = true;

            # buildInputs = with pkgs; [ ];
            #
            # nativeBuildInputs = with pkgs; [ ];
          };

          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              pname = "deps";
            }
          );
        in
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [
              inputs.rust-overlay.overlays.default
            ];
          };

          treefmt = {
            projectRootFile = "flake.nix";

            programs = {
              nixfmt.enable = true;
              rustfmt = {
                enable = true;
                package = rust-bin;
              };
              actionlint.enable = true;
            };
          };

          pre-commit = {
            settings = {
              hooks = {
                flake-treefmt = {
                  enable = true;
                  name = "flake-treefmt";
                  entry = lib.getExe config.treefmt.build.wrapper;
                  pass_filenames = false;
                };

                # clippy.enable = true;
                cargo-check.enable = true;

                clippy.packageOverrides.cargo = rust-bin;
                clippy.packageOverrides.clippy = rust-bin;
              };

              settings.rust.check.cargoDeps = pkgs.rustPlatform.importCargoLock {
                lockFile = ./Cargo.lock;
              };
            };
          };

          devShells.default = pkgs.mkShell {
            inputsFrom = [ config.pre-commit.devShell ];

            buildInputs = with pkgs; [
              cargo-expand
              cargo-nextest

              rust-bin
            ];
          };

          packages = rec {
            default = sand;
            sand = craneLib.buildPackage (
              commonArgs
              // {
                inherit cargoArtifacts;
                pname = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.name;
                version = "unstable-${self.shortRev or "dirty"}";
              }
            );

            ci = pkgs.buildEnv {
              name = "ci";
              paths = with pkgs; [
                rust-bin

                typos
                cargo-nextest
              ];
            };
          };
        };
    };
}
