# Third-party notices

NinjaCat is licensed under the AGPL-3.0 (`LICENSE` at the repository root). Some of the files
under `backend/` are copied or generated from other projects and remain under those projects'
licences, listed here.

| component | upstream | version | licence | copyright | our files |
|-----------|----------|---------|---------|-----------|-----------|
| datadog-agent | github.com/DataDog/datadog-agent | `pkg/proto` v0.83.2; `sds_result.proto` from tag 7.84.0 | Apache-2.0 | Copyright 2016-present Datadog, Inc. | `backend/proto/protos/datadog-agent/` (copied); `backend/intake/src/ddsketch.rs` (ported from `pkg/util/quantile/config.go`) |
| agent-payload | github.com/DataDog/agent-payload | v5.0.207 | BSD-3-Clause | Copyright (c) 2017, Datadog, Inc. | `backend/proto/protos/agent-payload/` (copied, one edit, see `backend/proto/protos/README.md`); `backend/intake/src/process_frame.rs` (ported from `process/message.go`); `backend/intake/src/routers/process_buffers.rs` (ported from `process/tags.go`, `tags_v2.go`, `dns_v1.go`, `dns_v2.go`) |
| sketches-go | github.com/DataDog/sketches-go | v1.4.8 | Apache-2.0 | Copyright 2021 Datadog, Inc. | `backend/proto/protos/sketches-go/` (copied) |
| pprof | github.com/google/pprof | v0.0.0-20260906184651-6331bc6350fe | Apache-2.0 | Copyright 2016 Google Inc. | `backend/proto/protos/pprof/` (copied) |

`backend/proto/src/generated/` is generated from all four (`cargo run -p protogen`) and is
under the same licences as the `.proto` files it comes from.

## Licence texts

The full text of each licence is kept beside the files it covers:

- Apache-2.0, datadog-agent: `backend/proto/protos/datadog-agent/LICENSE`
- Apache-2.0, sketches-go: `backend/proto/protos/sketches-go/LICENSE`
- Apache-2.0, pprof: `backend/proto/protos/pprof/LICENSE`
- BSD-3-Clause, agent-payload: `backend/proto/protos/agent-payload/LICENSE`

## NOTICE files

Apache-2.0 section 4(d) asks that a NOTICE file of the original be passed on. Two of the
upstream projects have one; each is also kept beside its LICENSE
(`backend/proto/protos/datadog-agent/NOTICE`, `backend/proto/protos/sketches-go/NOTICE`).
pprof has none.

### datadog-agent

```
Datadog datadog-agent
Copyright 2016-present Datadog, Inc.

This product includes software developed at Datadog (https://www.datadoghq.com/).
This product includes software developed at CoreOS, Inc (http://www.coreos.com/).
```

### sketches-go

```
Datadog sketches-go
Copyright 2021 Datadog, Inc.

This product includes software developed at Datadog (https://www.datadoghq.com/).
```
