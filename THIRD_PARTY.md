# Third-party notices

## botocore service models

`crates/emu-core/assets/catalog.json.gz` is generated from the service models
shipped with [botocore](https://github.com/boto/botocore), the Python library
behind the AWS CLI. The models are Copyright Amazon.com, Inc. or its
affiliates and are licensed under the Apache License, Version 2.0.

Only the routing metadata needed by this TUI is retained: service names,
protocols, SigV4 signing names, operation names, required-input skeletons, and
HTTP bindings. Documentation, examples, and output shapes are discarded. The
generator is `scripts/generate-catalog.py`.

The upstream attribution notice is reproduced in
`LICENSES/botocore-NOTICE`, and a copy of the Apache License 2.0 is in
`LICENSES/Apache-2.0.txt`.

## AWS Emulator

This project talks to [aws/aws-emulator](https://github.com/aws/aws-emulator),
which is licensed CC0-1.0. No emulator code is vendored here; the TUI is an
ordinary AWS API client.