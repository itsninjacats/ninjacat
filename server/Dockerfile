# Production image for the server, built in two stages.
#
# CGO_ENABLED=1 is mandatory, not a preference: agent-payload/process unpacks
# the process-agent's legacy zstd frames through DataDog/zstd_0, which is a
# cgo binding. A CGO_ENABLED=0 build fails to link.
#
# Because of that the runtime stage needs a libc, so it is debian-slim rather
# than scratch or distroless-static.
FROM golang:1.26 AS builder

WORKDIR /src

# Dependencies first: this layer is reused for as long as go.mod and go.sum
# are untouched, which is most rebuilds.
COPY go.mod go.sum ./
RUN go mod download

COPY . .

# -trimpath keeps build machine paths out of the binary; -s -w drop the
# symbol table and DWARF, which is a large share of a Go binary's size.
RUN CGO_ENABLED=1 go build -trimpath -ldflags="-s -w" -o /out/ninjacat ./cmd/ninjacat

FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --create-home --uid 10001 ninjacat

COPY --from=builder /out/ninjacat /usr/local/bin/ninjacat

WORKDIR /data
RUN chown ninjacat:ninjacat /data
USER ninjacat

# 8080 agent intake, 8081 panel API, 9911 Ergo observer.
EXPOSE 8080 8081 9911

ENTRYPOINT ["ninjacat"]
