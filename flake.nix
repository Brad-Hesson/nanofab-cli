{
  description = "A CLI program for accessing the Univeristy of Alberta Nanofabrication Laboratory user portal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = flakes: flakes.flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import flakes.nixpkgs { inherit system; };
      fenix = flakes.fenix.packages.${system};
      crane = (flakes.crane.mkLib pkgs).overrideToolchain (fenix.combine [
        fenix.stable.defaultToolchain
        fenix.stable.rust-src
      ]);
      crateArgs = {
        src = crane.cleanCargoSource ./.;
        strictDeps = true;
        buildInputs = [ pkgs.openssl.dev ];
        nativeBuildInputs = [ pkgs.pkg-config ];
      };
      cargoArtifacts = crane.buildDepsOnly crateArgs;
      crate = crane.buildPackage (crateArgs // { inherit cargoArtifacts; });
    in
    {
      checks = {
        inherit crate;
        clippy = crane.cargoClippy (crateArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        });
        documentation = crane.cargoDoc (crateArgs // {
          inherit cargoArtifacts;
        });
        formatting = crane.cargoFmt { inherit (crateArgs) src; };
      };
      packages.default = crate;
      apps.default = (flakes.flake-utils.lib.mkApp { drv = crate; }) // {
        meta.description = "A CLI program for accessing the Univeristy of Alberta Nanofabrication Laboratory user portal";
      };
      devShells.default = crane.devShell {
        inputsFrom = [ crate ];
        packages = with pkgs; [
          nixpkgs-fmt
          nixd
        ];
      };
    }
  );
}
