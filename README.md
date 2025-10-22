# Pubky Homeserver CLI

A Rust-based command line companion for interacting with Pubky homeservers. It wraps both the
admin and user-facing APIs, reusing the official `pubky` SDK (`0.6.0-rc.6`) and the
`pubky-testnet` harness so you can automate local testing or drive a real deployment from scripts.

## Features

- **User flows** - signup, signin, session inspection, signout, directory listing, and third-party auth token hand-off.
- **Admin flows** - generate invite tokens, gather server stats, enable/disable users, and delete WebDAV entries.
- **Integration testing** - comprehensive end-to-end tests spin up an ephemeral testnet and call the real CLI via `assert_cmd`.
- **Continuous integration** - GitHub Actions workflow runs format checks and the full test suite on every push/PR.

## Requirements

- Rust toolchain (1.76 or newer is recommended)
- `cargo` (ships with Rust)
- For local development: a running homeserver (e.g. `cargo run -p pubky-homeserver -- --data-dir ~/.pubky`) or the `pubky-testnet` binary.

## Quick Start

```bash
# Clone and build
cargo build

# Run the CLI (examples assume the admin API listens on http://127.0.0.1:6288)
PUBKY_ADMIN_PASSWORD=admin cargo run -- admin info

# Signup a user (passphrase entered interactively unless the env var below is set)
PUBKY_CLI_RECOVERY_PASSPHRASE=pass cargo run -- user signup <homeserver-pk> ./alice.recovery --testnet
```

### Environment Variables

| Variable                       | Purpose                                                                  |
|-------------------------------|--------------------------------------------------------------------------|
| `PUBKY_ADMIN_PASSWORD`        | Password passed to admin endpoints; can be set globally instead of `--password`. |
| `PUBKY_CLI_RECOVERY_PASSPHRASE` | Optional passphrase to automatically decrypt recovery files (useful in CI/tests). |

### Running Tests

```bash
# Unit + end-to-end tests (uses pubky-testnet internally)
cargo test
```

The integration tests create temporary recovery files and launch a sandboxed Pubky testnet, so no additional setup is required.

## Continuous Integration

The repo ships with `.github/workflows/ci.yml`. Each run performs:

1. `cargo fmt --check`
2. `cargo test --all`

Caching is enabled for the cargo registry, git index, and the `target` directory to keep the workflow fast.

## Project Layout

```
├── src/
│   ├── admin.rs    # admin subcommands + HTTP wrapper
│   ├── user.rs     # user subcommands built on the pubky SDK
│   ├── util.rs     # shared helpers (builders, recovery-file handling)
│   └── main.rs     # thin entrypoint wiring clap + modules
├── tests/
│   └── integration.rs  # admin + user e2e coverage using pubky-testnet
├── Cargo.toml
├── LICENSE
└── README.md
```

## Useful References

- [Pubky SDK docs](https://docs.rs/pubky/0.6.0-rc.6/pubky/)
- [pubky-core repository](https://github.com/pubky/pubky-core)

Feedback and contributions are welcome—feel free to open issues or PRs!***
