#!/usr/bin/env python3
"""记录 release-smoke CI 的构建输入（schema kiana.build-inputs.v1）。

由 .github/workflows/release-smoke.yml 调用；依赖调用方先 export 三个环境变量：
TOOLCHAIN_VERSION / CARGO_LOCK_HASH / SOURCE_HASH。
"""
import datetime
import json
import os
import pathlib

record = {
    "schema": "kiana.build-inputs.v1",
    "toolchain_channel": "stable",
    "toolchain_version": os.environ["TOOLCHAIN_VERSION"],
    "cargo_lock_hash": os.environ["CARGO_LOCK_HASH"],
    "source_hash": os.environ["SOURCE_HASH"],
    "build_timestamp": datetime.datetime.now(datetime.timezone.utc).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    ),
    "ci_run_id": os.environ.get("GITHUB_RUN_ID", ""),
}

pathlib.Path("dist/build-inputs.json").write_text(
    json.dumps(record, indent=2, sort_keys=True) + "\n"
)
print(
    "build-inputs recorded: toolchain={} source={}".format(
        record["toolchain_version"], record["source_hash"][:8]
    )
)
