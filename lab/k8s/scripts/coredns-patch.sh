#!/usr/bin/env bash
# Points every *.ninjacat.lab name at the lab's TLS proxy.
#
# This is the piece that makes DD_SITE work the way it does against the real
# Datadog. The agent composes app.ninjacat.lab, kubeops-intake.ninjacat.lab,
# contlcycle-intake.ninjacat.lab and three dozen more entirely on its own; we
# never enumerate them. CoreDNS collapses the whole space onto one Service,
# and the Host header — which DNS does not touch — still tells the intake
# which product each request belongs to.
#
# `answer auto` rewrites the name back in the reply, so the client sees an
# answer for the name it actually asked about.
#
# The regex is anchored as ^...\.$ for two reasons, both learned the hard way:
#
#  - CoreDNS matches the QNAME, which is fully qualified and therefore ends in
#    a dot. A pattern ending in `lab$` matches nothing at all, and the symptom
#    is the agent reporting "no such host" for every endpoint while CoreDNS
#    reports no error whatsoever.
#  - Leaving the pattern unanchored would also match the resolver's search-path
#    probes (app.ninjacat.lab.svc.cluster.local. and friends, because ndots:5
#    makes the stub try those first), rewriting lookups that were never meant
#    for us.
set -euo pipefail
CTX="${1:?usage: coredns-patch.sh <kube-context>}"
TARGET="ninjacat-proxy.ninjacat-lab.svc.cluster.local"

current=$(kubectl --context "$CTX" -n kube-system get configmap coredns -o jsonpath='{.data.Corefile}')

# Strip any previous version of our block before inserting, so fixing the rule
# is a matter of re-running this rather than hand-editing a live ConfigMap.
current=$(awk '
  /^[[:space:]]*rewrite stop \{/ { buf = $0 "\n"; inblk = 1; next }
  inblk { buf = buf $0 "\n"; if ($0 ~ /^[[:space:]]*\}/) { inblk = 0; if (buf !~ /ninjacat\.lab/) printf "%s", buf; buf = "" } ; next }
  { print }
' <<<"$current")

patched=$(awk -v target="$TARGET" '
  /^[[:space:]]*kubernetes[[:space:]]+cluster\.local/ && !done {
    print "    rewrite stop {"
    print "        name regex ^(.*)\\.ninjacat\\.lab\\.$ " target
    print "        answer auto"
    print "    }"
    done = 1
  }
  { print }
' <<<"$current")

kubectl --context "$CTX" -n kube-system create configmap coredns \
  --from-literal=Corefile="$patched" --dry-run=client -o yaml \
  | kubectl --context "$CTX" apply -f - >/dev/null

kubectl --context "$CTX" -n kube-system rollout restart deployment/coredns >/dev/null
kubectl --context "$CTX" -n kube-system rollout status deployment/coredns --timeout=90s
echo "coredns: *.ninjacat.lab -> $TARGET"
