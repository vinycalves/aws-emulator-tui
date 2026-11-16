# Launch copy (English)

## Repository description (short)

> A terminal console for AWS Emulator — browse 436 services, drill into
> resources, invoke any operation, and manage the emulator, from the terminal.
> Built in Rust.

## LinkedIn / long post

I built **emutui** — an open-source terminal console for AWS Emulator.

AWS Emulator gives you the AWS APIs locally, but no console. You create an S3
bucket with one CLI command, a DynamoDB table with another, and then have to
remember how to inspect them. emutui gives that local cloud a terminal UI.

What it does:

- **Explorer** — live resource tables for 16 services, with create/delete and
  drill-downs: DynamoDB table → items, SQS queue → messages, CloudFormation
  stack → resources, log group → streams, ECS cluster → services.
- **Browse** — any of the 436 services and 19,607 operations. It finds the
  records in the response, derives the columns, and follows `NextToken`,
  `Marker`, and `LastEvaluatedKey` for pagination.
- **API console** — protocol-aware requests (JSON, query, EC2, REST-JSON),
  SigV4-signed with the keys the emulator generates.
- **Emulator lifecycle** — start, stop, restart, health checks, JSON log tail,
  and telemetry disabled for processes it starts.
- **Daily-use tools** — diagnostics, read-only mode, command palette, JSON
  export, OSC52 copy, and switching between local emulator instances.
- **One Rust binary** — no browser, no Python runtime, no Node, no Docker. It
  works over SSH and reads the emulator's own endpoint, credentials, and CA.

Built with Rust, Ratatui, Tokio, reqwest, and a catalog generated from
botocore's service models. MIT licensed.

Repo: https://github.com/vinycalves/aws-emulator-tui

#Rust #AWS #OpenSource #TUI #DeveloperTools #CloudNative #AWSCommunity

## X / short post

I built emutui — a terminal console for AWS Emulator.

Browse 436 AWS services, drill into DynamoDB items / SQS messages / CFN
resources, invoke any operation, and start/stop the emulator — all from the
terminal. One Rust binary, SigV4, MIT.

https://github.com/vinycalves/aws-emulator-tui

## Show HN

**Show HN: emutui – a terminal console for AWS Emulator**

AWS Emulator is a local emulator of the AWS APIs: fast, offline, no account.
But it has no UI, so inspecting what you created means another CLI call.

emutui is a TUI that attaches to it as a normal AWS client. It discovers the
emulator's endpoint, keys, and certificate from `~/.aws/emulator/<port>/`,
signs every request with SigV4, and gives you:

- live tables for 16 services with create/delete and child-resource drill-downs
- a generic browser over 436 services and 19,607 operations
- a protocol-aware API console
- start/stop/restart, health checks, and log tail from the same terminal
- diagnostics, read-only mode, export, copy, and endpoint switching

It is a ~5.7 MB Rust binary (Ratatui + Tokio), MIT licensed, and it does not
fork or embed the emulator.

Repo: https://github.com/vinycalves/aws-emulator-tui

I would love feedback on the generic table extraction and on which service
deserves the next hand-written view.