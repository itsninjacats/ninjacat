# Budowa ninjacata. CGO musi byc wlaczone, bo agent-payload/process uzywa
# DataDog/zstd_0 do rozpakowania ramek process-agenta (legacy zstd).
FROM golang:1.26 AS builder
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=1 go build -o /out/ninjacat ./cmd/ninjacat

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/ninjacat /usr/local/bin/ninjacat
WORKDIR /data
EXPOSE 8080
ENTRYPOINT ["ninjacat"]
