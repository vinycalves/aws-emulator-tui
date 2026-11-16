# Changelog

All notable changes to this project are documented here.

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