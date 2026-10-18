# Third-party notices

NinjaCat is licensed under the AGPL-3.0 (`LICENSE`). Some of its files are copied or
ported from other projects and remain under those projects' licences, listed here.

| component | upstream | version | licence | copyright | our files |
|-----------|----------|---------|---------|-----------|-----------|
| datadog-agent | github.com/DataDog/datadog-agent | `pkg/proto` v0.83.2; `sds_result.proto` from tag 7.84.0 | Apache-2.0 | Copyright 2016-present Datadog, Inc. | `api/src/Proto/protos/datadog-agent/` (copied); `api/src/Intake/DDSketch.fs` (ported from `pkg/util/quantile/config.go`) |
| agent-payload | github.com/DataDog/agent-payload | v5.0.207 | BSD-3-Clause | Copyright (c) 2017, Datadog, Inc. | `api/src/Proto/protos/agent-payload/` (copied, one edit, see `api/src/Proto/protos/README.md`); `api/src/Intake/ProcessFrame.fs` (ported from `process/message.go`, version not recorded); `api/src/Intake/Routers/ProcessBuffers.fs` (ported from `process/tags.go`, `tags_v2.go`, `dns_v1.go`, `dns_v2.go`) |
| sketches-go | github.com/DataDog/sketches-go | v1.4.8 | Apache-2.0 | Copyright 2021 Datadog, Inc. | `api/src/Proto/protos/sketches-go/` (copied) |
| pprof | github.com/google/pprof | v0.0.0-20260906184651-6331bc6350fe | Apache-2.0 | Copyright 2016 Google Inc. | `api/src/Proto/protos/pprof/` (copied) |
| statsmodels | github.com/statsmodels/statsmodels | 0.15.0 | BSD-3-Clause | Copyright (c) 2009-2018 statsmodels Developers, and others | `api/src/Engine/Query/Stl.fs` (ported from `statsmodels/tsa/stl/_stl.pyx`) |

## Licence texts

The full text of each licence is kept beside the files it covers:

- Apache-2.0, datadog-agent: `api/src/Proto/protos/datadog-agent/LICENSE`
- Apache-2.0, sketches-go: `api/src/Proto/protos/sketches-go/LICENSE`
- Apache-2.0, pprof: `api/src/Proto/protos/pprof/LICENSE`
- BSD-3-Clause, agent-payload: `api/src/Proto/protos/agent-payload/LICENSE`
- BSD-3-Clause, statsmodels: in the header of `api/src/Engine/Query/Stl.fs`

## NOTICE files

Apache-2.0 section 4(d) asks that a NOTICE file of the original be passed on. Two of
the upstream projects have one; each is also kept beside its LICENSE
(`api/src/Proto/protos/datadog-agent/NOTICE`, `api/src/Proto/protos/sketches-go/NOTICE`).
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
