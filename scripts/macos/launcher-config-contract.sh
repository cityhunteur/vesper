#!/usr/bin/env bash
# TD-044: the launcher decodes the golden launch config (every field set; held equal to src/systemvm.rs by
# tests/launch_config_contract.rs) and prints it back; the two must be equal. A field the launcher drops or reads
# differently fails here. macOS only; needs `make vmlauncher-build`.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LAUNCHER="${LAUNCHER:-${ROOT_DIR}/build/vmlauncher/vesper-vmlauncher}"
GOLDEN="${GOLDEN:-${ROOT_DIR}/testdata/contracts/launch-config.full.json}"
decoded="$("${LAUNCHER}" --config "${GOLDEN}" --print-config)"
python3 -I - "${GOLDEN}" "${decoded}" <<'PY'
import json, sys
want = json.load(open(sys.argv[1])); got = json.loads(sys.argv[2])
missing = sorted(set(want) - set(got)); extra = sorted(set(got) - set(want))
differ = sorted(k for k in set(want) & set(got) if want[k] != got[k])
if missing or extra or differ:
    print(f"launcher-config-contract: dropped by the launcher {missing}; only in the launcher {extra}; read differently {differ}")
    for k in differ:
        print(f"  {k}: config {json.dumps(want[k])} launcher {json.dumps(got[k])}")
    sys.exit(1)
print(f"launcher-config-contract: the launcher decodes all {len(want)} fields as written")
PY
