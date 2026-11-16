# Roadmap and scenario coverage

This is the working map of everything someone might reasonably want to do with
a local AWS emulator, and where `emutui` stands today. It is deliberately more
ambitious than the current release: the goal is to make the terminal a complete
workbench, not just a resource viewer.

Legend: **✅ done** · **🟡 partial** · **⬜ planned**

## 1. First run and connection

| Scenario | Status | Notes |
| --- | --- | --- |
| Discover endpoint, keys, CA, region from `~/.aws/emulator/<port>/` | ✅ | No configuration required. |
| Attach to an emulator started elsewhere | ✅ | Health check + identity. |
| Start, stop, restart the emulator from the UI | ✅ | Telemetry disabled for TUI-started processes. |
| Tail the emulator's JSON logs | ✅ | Filtering only by eye today. |
| Diagnose a broken setup | ✅ | `D`: health, STS, files, permissions, binary version, probe summary. |
| Switch between multiple local emulators | ✅ | Scans `~/.aws/emulator/*/config`; history is not partitioned yet. |
| Start on an ephemeral port (`-a`, `--idle-timeout`, `-b`) | 🟡 | Port is discovered, other flags are not exposed in the UI. |
| Seed demo data from the UI | 🟡 | `scripts/seed-demo.sh` exists; not a UI action. |
| Persist preferences and last session | ⬜ | Endpoint, read-only, theme, last tab, history. |
| Guided setup/onboarding tour | ⬜ | Install guidance and a first-resource tutorial. |

## 2. Finding resources

| Scenario | Status | Notes |
| --- | --- | --- |
| Curated tables for common services | ✅ | 16 services. |
| Browse any of the 436 catalog services | ✅ | Generic table extraction, list-like operations first. |
| Follow pagination tokens | ✅ | Browse: `NextToken`, `Marker`, `NextContinuationToken`, `LastEvaluatedKey`. Curated tables: first page only. |
| Filter rows locally | ✅ | `/` on Explorer and Browse. |
| Inspect a row as JSON | ✅ | `Enter`; full detail is also kept in request history. |
| Drill into child resources | ✅ | DynamoDB items, SQS messages, CFN resources, log streams, ECS services, ECR images, SNS subscriptions, IAM policies, KMS grants, SFN executions, EventBridge rules. |
| Cross-service resource search | ⬜ | Resource Groups Tagging API / Resource Explorer, plus local index. |
| Favorites, bookmarks, recent views | ⬜ | Per-endpoint. |
| Sort columns, resize, hide columns | ⬜ | Useful on the 8–10 column generic tables. |
| Auto-refresh with an interval | ⬜ | Useful for logs and queues. |
| Multi-select and bulk delete | ⬜ | Needs a selection model beyond one row. |
| Tag display and editing across services | ⬜ | Read tags first, then write where supported. |
| Cross-endpoint diff | ⬜ | Compare two emulators, or an emulator with a saved state. |

## 3. Acting on resources

| Scenario | Status | Notes |
| --- | --- | --- |
| Create/delete curated resources | ✅ | Buckets, tables, queues, topics, keys, secrets, parameters, stacks, log groups, buses, clusters, repositories, state machines. |
| S3: create bucket, list, delete, preview text | ✅ | Preview falls back to a download hint for binary objects. |
| S3: upload, download, copy, presign, folders | ⬜ | High priority; needs a local-file picker/path input and binary transport. |
| DynamoDB: browse items | ✅ | Via `Scan` drill-down, with typed attribute decoration. |
| DynamoDB: add/edit/delete an item visually | ⬜ | Current path is `Console`/`Browse` with `PutItem`/`DeleteItem` JSON. |
| DynamoDB: query by key/index, not just scan | ⬜ | Needs key schema from `DescribeTable` and a query builder. |
| SQS: receive messages | ✅ | Drill-down `ReceiveMessage` (does not delete). |
| SQS: send message | ✅ | `x` on a queue. |
| SQS: delete/purge/attributes | 🟡 | Reachable through Browse operations; not one keystroke yet. |
| SNS: publish, subscriptions | ✅ / 🟡 | Publish via `x`; subscriptions via `o`. |
| Lambda: invoke with payload | ✅ | `x`, response in a scrollable modal. |
| Lambda: edit config, aliases, versions, logs | ⬜ | Config can be reached through Browse/Console. |
| Step Functions: start execution | ✅ | `x` with the standard input JSON. |
| Step Functions: execution history/graph | ⬜ | Very valuable; needs ASL rendering. |
| CloudFormation: inspect resources, delete | ✅ | Create/update from a template is not exposed yet. |
| CloudWatch Logs: list groups/streams | ✅ | Drill-down to streams. |
| CloudWatch Logs: tail and filter events | ⬜ | Needs `GetLogEvents` + streaming. |
| Secrets Manager: reveal/update | 🟡 | Detail call exists; reveal is not masked by default yet. |
| SSM: get/put parameter | 🟡 | Browse/Console; no dedicated editor. |
| IAM: read users/roles/policies | ✅ | Writes intentionally not offered. |
| EC2: start/stop/terminate | ⬜ | Read-only today. |
| Schema-driven create/edit forms | ⬜ | Today's forms are JSON editors; botocore models can drive fields/enums. |
| Input validation before sending | ⬜ | Only JSON syntax is checked; the emulator validates semantics. |
| Typed confirmation for destructive actions | ⬜ | Current confirm is `y`/`Enter`. |

## 4. Any API operation

| Scenario | Status | Notes |
| --- | --- | --- |
| Invoke any of 19,607 operations | ✅ | Console, with the required-input skeleton. |
| Protocol-aware request shaping | ✅ | JSON 1.0/1.1, query, EC2, REST-JSON; S3 REST-XML via its screens. |
| Form view generated from the model | ⬜ | Would replace hand-edited JSON for query/EC2. |
| Pagination in the Console | 🟡 | Browse has it; Console runs one page. |
| Request presets and collections | ⬜ | Save, name, rerun, share. |
| Replay a request from history | ⬜ | History stores the bodies; one keystroke to re-run is next. |
| Copy as AWS CLI / boto3 / curl | ⬜ | Generate the equivalent command with endpoint and CA setup. |
| Cancel an in-flight request | ⬜ | Requests are short today, but `Describe*` on large states can be slow. |
| Compare two responses | ⬜ | Useful while iterating on emulator fidelity. |
| Request/response size guards | 🟡 | UI truncates long bodies; no explicit warning at the limit. |

## 5. Observability

| Scenario | Status | Notes |
| --- | --- | --- |
| Request history with status and duration | ✅ | In-memory, per session, capped at 200. |
| Inspect request and response bodies | ✅ | `Enter` on Requests. |
| Filter history by service/status/errors | ⬜ | Next after persisting history. |
| Latency statistics (p50/p95, per service) | ⬜ | |
| Export history as JSON/HAR | ⬜ | |
| CloudTrail event history | ⬜ | The emulator supports parts of it; verify fidelity first. |
| Log tail with search/highlight | ⬜ | Emulator process logs and CloudWatch Logs. |
| Crash detection and last-error report | 🟡 | Child exit is detected; no automatic restart policy. |

## 6. Safety and secrets

| Scenario | Status | Notes |
| --- | --- | --- |
| Never display credentials | ✅ | Keys are read from disk, never shown. |
| Never store Authorization headers | ✅ | History stores bodies only. |
| CA pinning for the emulator | ✅ | Uses the generated `ca.pem`. |
| Read-only mode | ✅ | Prefix heuristic; blocks create/delete/console/Browse writes. |
| Mask secret values by default | ⬜ | Reveal with a key press; redact history. |
| Typed confirmation for delete/purge | ⬜ | |
| Endpoint allowlist / guardrails | ⬜ | For teams running multiple emulators. |
| Clear all resources with one action | ⬜ | Restart is the current reset. |

## 7. State and reproducibility

| Scenario | Status | Notes |
| --- | --- | --- |
| Reset the emulator | ✅ | Stop/start (state is in memory). |
| Export the current table | ✅ | JSON, with detail objects. |
| Export the whole environment | ⬜ | Replayable seed script or JSON bundle across curated services. |
| Import/snapshot restore | ⬜ | Emulator has no snapshot API; we can replay creates. |
| Reproduce a bug report | ⬜ | Bundle requests + responses + emulator version. |
| Seed from a scenario file | 🟡 | Shell script exists; a TUI action and JSON format are next. |

## 8. Terminal UX

| Scenario | Status | Notes |
| --- | --- | --- |
| Keyboard-first navigation, vim keys | ✅ | Arrows, `hjkl`, `g/G`, page keys. |
| Help overlay | ✅ | `?` / `F1`. |
| Command palette | ✅ | `Ctrl+P` / `:`. |
| Copy to local clipboard | ✅ | OSC52, works over SSH where supported. |
| Mouse support | ⬜ | Click rows, scroll panes. |
| Light theme / high contrast / no-color | ⬜ | |
| ASCII-only fallback | ⬜ | Glyphs are Unicode today. |
| Persist layout and filters | ⬜ | |
| Configurable keybindings | ⬜ | |
| Localization | ⬜ | English only. |

## 9. Automation and agents

| Scenario | Status | Notes |
| --- | --- | --- |
| `--check` without a TTY | ✅ | Probes curated services, exits non-zero only when unreachable. |
| JSON output | ⬜ | `--check --json`, `list --json`. |
| CLI subcommands (`list`, `describe`, `export`) | ⬜ | Reuse `emu-core` outside the TUI. |
| Seed/export scripts | 🟡 | `scripts/seed-demo.sh` shipped. |
| MCP server for coding agents | ⬜ | Expose browse/act/read operations to an agent. |
| Non-interactive batch operations | ⬜ | JSONL input/output. |

## 10. Engineering quality

| Scenario | Status | Notes |
| --- | --- | --- |
| Unit tests for signatures, XML, generic extraction, tokens | ✅ | 23 tests today. |
| Live emulator CI | ✅ | Installs the package and runs `emutui --check`. |
| UI snapshot tests | ⬜ | Ratatui `TestBackend`. |
| Protocol golden tests | 🟡 | SigV4 vector + live probes. |
| Packaging (Homebrew, prebuilt binaries, Docker) | ⬜ | `cargo install` works today. |
| Reproducible catalog generation | ✅ | `scripts/generate-catalog.py`, gzip with `mtime=0`. |

## Suggested next three

1. **DynamoDB item editor** (scan/query + add/edit/delete) — the most common
   reason to open a local AWS console.
2. **S3 upload/download/presign** — the other most common reason.
3. **Persistent history, presets, and replay** — turns the Console into a
   daily-use debugging tool and feeds the automation work.

After that: CloudWatch Logs tail, environment export/restore, cross-service
search, schema-driven forms, and mouse/themes.

If you want to work on one of these, open an issue with the scenario and the
smallest useful slice. The architecture is intentionally split so `emu-core`
can grow without the TUI, and the TUI can grow without new protocol code.