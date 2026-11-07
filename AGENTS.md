# mdmc

Markdown mail compiler. Compiles markdown draft notes to RFC 5322 email, reusing
the markdown → pandoc → HTML/plain pipeline and MML body construction from the
Python `mail` tool, but reimplemented in Rust as a standalone CLI.

See `docs/spec.md` for the design.

## Development environment

Use `devenv` for all commands. The project pins its own toolchain and packages.

```sh
devenv shell
```

## Commands

- `mdmc <args>` — run the CLI via `cargo run`
- `build` — `cargo build --release`
- `tests` — `cargo test`

Direct checks (run inside the devenv shell):

```sh
cargo clippy
cargo fmt --check
nix flake check
```

## Conventions

- Version is declared in `Cargo.toml` only. `flake.nix` derives `pname` and `version` from it via `builtins.fromTOML`.
- Commit messages follow `<file>: <lowercase description>`.
- The design lives in `docs/spec.md`; update it alongside code changes.
