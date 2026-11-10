{
  description = "mdmc — markdown mail compiler";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      mdmc-pkg = pkgs.rustPlatform.buildRustPackage {
        pname = cargoToml.package.name;
        version = cargoToml.package.version;
        src = ./.;
        cargoLock = { lockFile = ./Cargo.lock; };
        nativeBuildInputs = [ pkgs.installShellFiles ];
        doCheck = false;
        postInstall = ''
          installManPage man/man1/*.1
        '';
      };
    in
    {
      packages.${system} = {
        default = mdmc-pkg;
        mdmc = mdmc-pkg;
      };

      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [ cargo rustc rustfmt clippy pandoc ];
      };
    };
}
