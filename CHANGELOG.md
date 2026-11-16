# Changelog

All notable changes to this project are documented here.

## 0.2.0 — 2026-11-16

- New **Browse** tab: a generic, model-free resource browser for all 436
  services, with list-like operations first, JSON input, filtering, row detail,
  and `NextToken`/`Marker`/`LastEvaluatedKey` pagination.
- Drill-downs from curated rows: DynamoDB items, SQS messages, CloudFormation
  resources, log streams, ECS services, ECR images, SNS subscriptions, IAM
  policies, KMS grants, SFN executions, EventBridge rules, Lambda event source
  mappings.
- Per-row actions: SQS send, SNS publish, Lambda invoke, Step Functions start.
- S3 text-object preview (with a download hint for binary objects).
- `D` diagnostics: health, identity, files, permissions, binary version, and
  probe summary.
- `Ctrl+P` / `:` command palette.
- Read-only mode, JSON export of the current table, and OSC52 copy.
- Emulator endpoint switcher across instances under `~/.aws/emulator/`.
- Error hints for 501/403/404/transport failures.
- Fixed the curated SNS view to parse the query/XML response.
- Generic tables order columns by usefulness (id, name, ARN, URL) and render
  DynamoDB attribute values and nested objects readably.

## 0.1.0 — 2026-11-16

First release.

- Explorer for S3, DynamoDB, SQS, SNS, Lambda, EC2, ECS, ECR, IAM, KMS,
  Secrets Manager, SSM, CloudFormation, CloudWatch Logs, EventBridge, and
  Step Functions.
- Create, inspect, and delete actions for the resources those services expose.
- A console for invoking any of the 19,600+ operations in the embedded
  436-service catalog, with protocol-aware request shaping for JSON, query,
  EC2, and REST-JSON services.
- Emulator supervisor: start, stop, restart, health checks, and JSON log tail.
- Request history with request/response detail.
- `--check` mode for scripts and CI.