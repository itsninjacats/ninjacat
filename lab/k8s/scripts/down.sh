#!/usr/bin/env bash
# Deletes the lab cluster. Nothing outside kind is touched.
set -euo pipefail
CLUSTER=ninjacat-lab
if kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  kind delete cluster --name "$CLUSTER"
  echo "lab cluster deleted"
else
  echo "no cluster named $CLUSTER"
fi
rm -rf "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.certs"
echo "lab certificates removed"
