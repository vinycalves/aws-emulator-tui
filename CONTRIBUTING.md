# Contributing

Thanks for helping. This project is a terminal client for AWS Emulator; it
does not modify the emulator itself.

## Development setup

You need a Rust toolchain (stable), and ideally a running emulator:

```bash
# Linux package
curl -fsSLo aws-emulator.deb \
  https://downloads.aws.com/aws-emulator/packages/aws-emulator/LTS/latest/aws-emulator_amd64.deb
sudo apt-get install -y ./aws-emulator.deb
aws-emulator &

git clone <your-fork>
cd aws-emulator-tui
cargo run -p emu-tui
```

The TUI discovers the endpoint, credentials, and CA certificate under
`~/.aws/emulator/9297/`. Use `--endpoint`, `EMULATOR_ENDPOINT`, or
`AWS_EMULATOR_BIN` to point it elsewhere.

## Before opening a pull request

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
HOME=$HOME cargo run -p emu-tui -- --check   # with an emulator running
```

All four are expected to pass. The repository's CI runs the same commands.

## Adding a resource view

1. Add a variant to `ViewId` in `crates/emu-core/src/services.rs`.
2. Implement its list call and row parsing in `fetch`.
3. Add it to `CURATED`, `base_view`, `list_operation`, and — if the service
   supports them — `create_kind`, `describe_call`, `build_create`, and
   `build_delete`.
4. Run the live smoke test:

   ```bash
   HOME=$HOME cargo run -p emu-core --example smoke
   ```

5. Add a row for it to the README.

All services are already reachable through the Console, even without a curated
view. A view is only needed for a table with friendly columns and actions.

## Regenerating the catalog

The catalog is generated from the botocore service models:

```bash
python3 -m pip install botocore
HOME=$HOME python3 scripts/generate-catalog.py
```

The script rewrites `crates/emu-core/assets/catalog.json.gz` and prints the
service/operation counts. Commit the result together with the botocore version
in the script header.

## Code style

- Keep `emu-core` free of UI code; it is usable from other front ends.
- Do not log or display credentials. Request history stores bodies, never
  `Authorization` headers.
- Errors shown in the UI should include the emulator's error code and message.
- Prefer adding a protocol feature to the client over hard-coding one service.

## Reporting fidelity gaps

If a call fails in a way that looks like an emulator gap rather than a TUI bug,
check the emulator's `issues/` directory and its release fidelity scores before
filing here. Include the request history entry (operation, status, body) from
the Requests tab.