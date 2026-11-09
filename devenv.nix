{ pkgs, config, ... }:

{
  languages.rust = {
    enable = true;
    channel = "nixpkgs";
  };

  packages = [
    pkgs.pandoc
    pkgs.notmuch
    pkgs.sqlite
  ];

  # Point notmuch at the local dev database (built by `dev-setup`).
  env.NOTMUCH_CONFIG = "${config.devenv.root}/.dev-state/notmuch-config";

  scripts.mdmc.exec = "cargo run -- --config \"${config.devenv.root}/.dev-state/mdmc.toml\" \"$@\"";
  scripts.build.exec = "cargo build --release";
  scripts.tests.exec = "cargo test";
  scripts.index.exec = "cargo run -- --config \"${config.devenv.root}/.dev-state/mdmc.toml\" index";
  scripts.notmuch-new.exec = "notmuch new";
  scripts.dev-setup.exec = "bash \"${config.devenv.root}/dev/setup.sh\"";
  scripts.dev-teardown.exec = "rm -rf \"${config.devenv.root}/.dev-state\"";

  enterShell = ''
    export MANPATH="${config.devenv.root}/man:''${MANPATH:-}"
    if [ ! -f "${config.devenv.root}/.dev-state/mdmc.toml" ]; then
      echo "mdmc — dev environment not set up; run 'dev-setup'"
    else
      echo "mdmc — local dev environment (.dev-state)"
    fi
  '';
}
