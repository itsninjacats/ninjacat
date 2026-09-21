#!/usr/bin/env bash
# Generates the lab CA and a wildcard certificate for *.ninjacat.lab.
#
# Why a CA at all: the Datadog agent composes https:// URLs from DD_SITE for
# every endpoint that has no dd_url of its own — container lifecycle, container
# images and the orchestrator among them. Those are precisely the payloads this
# lab exists to observe, so plain HTTP is not an option.
#
# Trusting a CA beats DD_SKIP_SSL_VALIDATION: the agent is written in Go, and
# Go's x509 honours SSL_CERT_FILE, so one mounted file makes every HTTP client
# inside the agent trust us at once — including the ones that never read the
# skip-validation setting.
set -euo pipefail
OUT="${1:?usage: certs.sh <output-dir>}"
mkdir -p "$OUT"

if [ -f "$OUT/ca.crt" ] && [ -f "$OUT/tls.crt" ]; then
  echo "certs: already present in $OUT, reusing"
  exit 0
fi

openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout "$OUT/ca.key" -out "$OUT/ca.crt" \
  -subj "/CN=NinjaCat Lab CA/O=NinjaCat Lab" 2>/dev/null

openssl req -newkey rsa:2048 -nodes \
  -keyout "$OUT/tls.key" -out "$OUT/tls.csr" \
  -subj "/CN=*.ninjacat.lab/O=NinjaCat Lab" 2>/dev/null

# The agent hits many different hostnames under the site, all one label deep
# (app.ninjacat.lab, kubeops-intake.ninjacat.lab, ...), so a single wildcard
# covers them. The bare apex is listed too because some probes use it.
cat > "$OUT/san.cnf" <<'SAN'
subjectAltName = DNS:*.ninjacat.lab, DNS:ninjacat.lab, DNS:*.logs.ninjacat.lab, DNS:*.agent.ninjacat.lab
extendedKeyUsage = serverAuth
SAN

openssl x509 -req -in "$OUT/tls.csr" -CA "$OUT/ca.crt" -CAkey "$OUT/ca.key" \
  -CAcreateserial -out "$OUT/tls.crt" -days 365 -extfile "$OUT/san.cnf" 2>/dev/null

rm -f "$OUT/tls.csr" "$OUT/san.cnf"
echo "certs: CA and wildcard cert for *.ninjacat.lab written to $OUT"
openssl x509 -in "$OUT/tls.crt" -noout -subject -ext subjectAltName | sed 's/^/  /'
