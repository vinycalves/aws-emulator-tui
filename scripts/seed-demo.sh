#!/usr/bin/env bash
# Seed a running AWS Emulator with a small, demo-friendly set of resources.
#
#   ./scripts/seed-demo.sh
#   EMULATOR_PORT=9298 ./scripts/seed-demo.sh
#
# Requires the AWS CLI v2 (any recent version) and a running emulator.
set -euo pipefail

PORT="${EMULATOR_PORT:-9297}"
REGION="${AWS_REGION:-us-east-1}"
STATE_DIR="${HOME}/.aws/emulator/${PORT}"

if [[ ! -f "${STATE_DIR}/emulator_credentials" ]]; then
  echo "No emulator credentials at ${STATE_DIR}." >&2
  echo "Start the emulator first (aws-emulator -p ${PORT}) or set EMULATOR_PORT." >&2
  exit 1
fi

export AWS_CONFIG_FILE="${STATE_DIR}/config"
export AWS_SHARED_CREDENTIALS_FILE="${STATE_DIR}/emulator_credentials"
export AWS_CA_BUNDLE="${STATE_DIR}/ca.pem"
export AWS_REGION="${REGION}"
export AWS_DEFAULT_REGION="${REGION}"

endpoint="https://127.0.0.1:${PORT}"
aws() { command aws --endpoint-url "${endpoint}" --region "${REGION}" "$@"; }

echo "Seeding ${endpoint}"

# S3: two buckets and one object.
aws s3api create-bucket --bucket demo-assets >/dev/null 2>&1 || true
aws s3api create-bucket --bucket demo-logs >/dev/null 2>&1 || true
tmp="$(mktemp)"
printf 'hello from emutui\n' > "${tmp}"
aws s3api put-object --bucket demo-assets --key welcome.txt --body "${tmp}" >/dev/null
rm -f "${tmp}"

# DynamoDB: one table with two items.
aws dynamodb create-table \
  --table-name orders \
  --attribute-definitions AttributeName=id,AttributeType=S \
  --key-schema AttributeName=id,KeyType=HASH \
  --billing-mode PAY_PER_REQUEST >/dev/null 2>&1 || true
# The emulator creates tables asynchronously, as DynamoDB does.
aws dynamodb wait table-exists --table-name orders >/dev/null 2>&1 || sleep 2
aws dynamodb put-item \
  --table-name orders \
  --item '{"id":{"S":"order-1001"},"status":{"S":"SHIPPED"},"total":{"N":"42.50"}}' >/dev/null
aws dynamodb put-item \
  --table-name orders \
  --item '{"id":{"S":"order-1002"},"status":{"S":"PENDING"},"total":{"N":"9.90"}}' >/dev/null

# Messaging.
aws sqs create-queue --queue-name jobs >/dev/null 2>&1 || true
aws sns create-topic --name alerts >/dev/null 2>&1 || true
aws events create-event-bus --name demo-bus >/dev/null 2>&1 || true

# Management and security.
aws logs create-log-group --log-group-name /aws/emutui/demo >/dev/null 2>&1 || true
aws secretsmanager create-secret --name demo/database \
  --secret-string '{"username":"demo","password":"not-a-real-secret"}' >/dev/null 2>&1 || true
aws ssm put-parameter --name /emutui/demo --value "hello" --type String >/dev/null 2>&1 || true

# Containers, keys, and a state machine.
aws ecr create-repository --repository-name demo/app >/dev/null 2>&1 || true
aws kms create-key --description "emutui demo key" >/dev/null 2>&1 || true
aws stepfunctions create-state-machine \
  --name demo-workflow \
  --definition '{"StartAt":"Done","States":{"Done":{"Type":"Succeed"}}}' \
  --role-arn "arn:aws:iam::000000000000:role/emulator" >/dev/null 2>&1 || true

echo "Done. Open the TUI and press 1 to browse, or a to reach any service."
echo "    emutui --endpoint ${endpoint}"