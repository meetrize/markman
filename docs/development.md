# Development & Build

This guide covers day-to-day development, building, testing, and packaging for Markman. The `scripts/` directory at the project root provides shell wrappers so you do not have to type long Cargo commands every time.

[English](development.md) | [中文](development.zh-CN.md)

## Prerequisites

- Git
- A Rust toolchain with Rust 2024 edition support
- Cargo
- Platform-native build dependencies required by GPUI and the system toolchain

## Run Modes

Markman is a GPUI desktop app and does **not** ship with built-in UI hot reload. Three common workflows:

| Mode | Script | Description |
| --- | --- | --- |
| Dev run | `./scripts/dev.sh` | Same as `cargo run`; debug build with fast incremental compiles. Heavy deps are optimized in dev via `[profile.dev.package]` in `Cargo.toml`. |
| Watch & restart | `./scripts/watch.sh` | Rebuild and restart on source changes (requires `cargo-watch`). |
| Release run | `./scripts/run.sh` | Run the release binary; builds first if the artifact is missing. |

> **Note:** `watch.sh` rebuilds and restarts the process on change. It is not a seamless hot reload like a web frontend. Unsaved editor state is lost on each restart.

## Scripts

```
scripts/
├── common.sh                  # Shared variables and helpers
├── dev.sh                     # Run in development mode
├── watch.sh                   # Watch sources and auto-restart
├── build.sh                   # Release build
├── run.sh                     # Run release binary
├── test.sh                    # Run tests
├── check.sh                   # Fast compile check
├── bench.sh                   # Run Criterion benchmarks
├── clean.sh                   # Clean target/ and dist/
├── package.sh                 # Platform packaging
├── create_macos_app_dist.sh   # Create macOS .app bundle
└── create_macos_pkg_dist.sh   # Create macOS PKG installer
```

Make scripts executable once:

```bash
chmod +x scripts/*.sh
```

## Daily Development

### Start the dev build

```bash
./scripts/dev.sh
./scripts/dev.sh test.md
./scripts/dev.sh -- --help
```

### Watch for changes

Install `cargo-watch` first:

```bash
cargo install cargo-watch
```

Then:

```bash
./scripts/watch.sh
./scripts/watch.sh test.md
```

Watched paths: `src/`, `assets/`, `resources/`, `build.rs`, and `Cargo.toml`.

### Quick compile check

```bash
./scripts/check.sh
```

## Build & Run

### Release build

```bash
./scripts/build.sh
./scripts/build.sh --locked   # lock deps, same as CI
```

Output: `target/release/markman` (or `markman.exe` on Windows).

### Run release binary

```bash
./scripts/run.sh
./scripts/run.sh test.md
./scripts/run.sh --detach     # macOS: launch in background
```

Equivalent manual commands:

```bash
cargo build --release
./target/release/markman
```

## Tests & Benchmarks

```bash
./scripts/test.sh
./scripts/test.sh editor::tests

./scripts/bench.sh
./scripts/bench.sh render_loop
```

## Clean

```bash
./scripts/clean.sh
```

Removes Cargo build artifacts and the local `dist/` directory.

## Packaging

```bash
./scripts/package.sh                  # auto-detect platform
./scripts/package.sh macos-app
./scripts/package.sh macos-pkg 0.5.7
./scripts/package.sh linux
./scripts/package.sh windows
```

### macOS step-by-step

```bash
./scripts/create_macos_app_dist.sh
./scripts/create_macos_pkg_dist.sh 0.5.7
```

Artifacts are written to `dist/`.

## CI Parity

The GitHub Actions workflow (`.github/workflows/build-release.yml`) runs `cargo build --release --locked` on each platform and packages zip, tar.gz, `.app`, or `.pkg` archives. Locally, use `./scripts/build.sh --locked` and `./scripts/package.sh` for a similar flow.

## FAQ

**Why does dev mode feel smoother than plain debug builds?**

`Cargo.toml` sets `opt-level = 3` for gpui and other heavy crates under `[profile.dev.package]`, keeping your own code debuggable while reducing framework overhead.

**Can I use Cargo directly?**

Yes. The scripts are convenience wrappers:

```bash
cargo run
cargo build --release
cargo test
cargo bench
```
