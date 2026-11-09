{ pkgs, ... }:

{
  languages.rust = {
    enable = true;
    channel = "nixpkgs";
  };

  packages = [
    pkgs.pandoc
  ];

  scripts.mdmc.exec = "cargo run -- \"$@\"";
  scripts.build.exec = "cargo build --release";
  scripts.tests.exec = "cargo test";

  enterShell = ''
    echo "mdmc — markdown mail compiler"
  '';
}
