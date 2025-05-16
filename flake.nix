{
  description = "Build a cargo project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = flakes: flakes.flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import flakes.nixpkgs { inherit system; };
      craneLib = flakes.crane.mkLib pkgs;
      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;
        buildInputs = [ pkgs.openssl.dev ];
        nativeBuildInputs = [ pkgs.pkg-config ];
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      crate = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
    in
    {
      packages.default = crate;
      apps.default = flakes.flake-utils.lib.mkApp { drv = crate; };
      devShells.default = craneLib.devShell {
        checks = { inherit crate; };
        packages = with pkgs; [
          nixpkgs-fmt
          nixd
        ];
      };
    }
  );
}
