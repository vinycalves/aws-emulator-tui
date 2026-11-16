#!/usr/bin/env python3
"""Generate the embedded service catalog from the botocore service models.

Usage:
    python3 -m pip install botocore
    python3 scripts/generate-catalog.py

Writes crates/emu-core/assets/catalog.json.gz. The catalog keeps only what the
TUI needs to route and shape a request:

  - service name, display name, protocol, API version, JSON target prefix,
    SigV4 signing name;
  - every operation name;
  - the required input members, resolved into a JSON skeleton;
  - the operation's HTTP method/URI and label/query/header bindings for REST
    protocols.

Documentation, examples, output shapes, and paginators are discarded.

botocore is Copyright Amazon.com, Inc. or its affiliates, Apache-2.0. See
THIRD_PARTY.md.
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path

try:
    import botocore
except ImportError:
    sys.exit("botocore is not installed: python3 -m pip install botocore")

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "crates" / "emu-core" / "assets" / "catalog.json.gz"
DATA = Path(botocore.__file__).resolve().parent / "data"


def load_model(key: str):
    candidates = sorted(DATA.glob(f"{key}/*/service-2.json*"))
    if not candidates:
        return None
    path = candidates[-1]
    if path.suffix == ".gz":
        with gzip.open(path, "rt", encoding="utf-8") as handle:
            return json.load(handle)
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def skeleton(shapes: dict, name: str | None, depth: int = 0, seen: set[str] | None = None):
    """Resolve required input members into a JSON skeleton, capped at depth 5."""
    if not name or depth > 5:
        return None
    if seen is None:
        seen = set()
    if name in seen:
        return None
    shape = shapes.get(name)
    if not shape:
        return None

    kind = shape.get("type")
    seen = seen | {name}

    if kind == "structure":
        required = set(shape.get("required", []))
        return {
            member_name: skeleton(shapes, member.get("shape"), depth + 1, seen)
            for member_name, member in shape.get("members", {}).items()
            if member_name in required
        }
    if kind == "list":
        item = skeleton(shapes, shape.get("member", {}).get("shape"), depth + 1, seen)
        return [] if item is None else [item]
    if kind == "map":
        value = skeleton(shapes, shape.get("value", {}).get("shape"), depth + 1, seen)
        return {} if value is None else {"key": value}

    return {
        "string": "",
        "integer": 0,
        "long": 0,
        "float": 0.0,
        "double": 0.0,
        "boolean": False,
        "timestamp": "2020-01-01T00:00:00Z",
        "blob": "",
    }.get(kind)


def operation_meta(shapes: dict, operation: dict) -> dict:
    input_shape = shapes.get(operation.get("input", {}).get("shape"), {})
    if input_shape.get("type") != "structure":
        return {"skel": {}}

    meta: dict = {"skel": skeleton(shapes, operation.get("input", {}).get("shape")) or {}}

    http = operation.get("http", {})
    if http:
        meta["method"] = http.get("method")
        meta["uri"] = http.get("requestUri")

    labels, query, headers = [], [], []
    payload = None
    for member_name, member in input_shape.get("members", {}).items():
        location = member.get("location")
        location_name = member.get("locationName", member_name)
        if location == "httpLabel":
            labels.append([member_name, location_name])
        elif location == "httpQuery":
            query.append([member_name, location_name])
        elif location == "httpHeader":
            headers.append([member_name, location_name])
        elif location == "httpPayload":
            payload = member_name

    if labels:
        meta["labels"] = labels
    if query:
        meta["queryMembers"] = query
    if headers:
        meta["headerMembers"] = headers
    if payload:
        meta["payload"] = payload
    return meta


def main() -> None:
    services = {}
    operations = 0

    for directory in sorted(DATA.iterdir()):
        if not directory.is_dir():
            continue
        model = load_model(directory.name)
        if not model or not model.get("operations"):
            continue

        metadata = model.get("metadata", {})
        ops = {
            name: operation_meta(model["shapes"], operation)
            for name, operation in sorted(model["operations"].items())
        }
        if not ops:
            continue

        services[directory.name] = {
            "display": metadata.get("serviceAbbreviation")
            or metadata.get("serviceFullName")
            or directory.name,
            "full": metadata.get("serviceFullName") or directory.name,
            "protocol": metadata.get("protocol"),
            "target": metadata.get("targetPrefix"),
            "version": metadata.get("apiVersion"),
            "signing": metadata.get("signingName")
            or metadata.get("endpointPrefix")
            or directory.name,
            "service_id": metadata.get("serviceId"),
            "json_version": metadata.get("jsonVersion"),
            "query_compatible": metadata.get("awsQueryCompatible") is not None,
            "ops": ops,
        }
        operations += len(ops)

    payload = {
        "source": f"botocore {botocore.__version__}",
        "services": services,
    }
    encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT, "wb") as handle:
        handle.write(gzip.compress(encoded, 9, mtime=0))

    print(f"botocore       {botocore.__version__}")
    print(f"services       {len(services)}")
    print(f"operations     {operations}")
    print(f"json           {len(encoded) / 1_000_000:.2f} MB")
    print(f"gzip           {OUTPUT.stat().st_size / 1000:.0f} kB")
    print(f"written        {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()