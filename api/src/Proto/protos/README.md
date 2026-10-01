# Datadog's protobuf definitions

The `.proto` files the Datadog Agent's payloads are defined by, copied
unmodified from Datadog's and Google's repositories, each under its own
licence (the `LICENSE` file beside it):

| directory        | source                                                 | version                              | licence      |
|------------------|--------------------------------------------------------|--------------------------------------|--------------|
| `agent-payload/` | github.com/DataDog/agent-payload                       | v5.0.207                             | BSD-3-Clause |
| `datadog-agent/` | github.com/DataDog/datadog-agent, `pkg/proto`          | v0.83.2                              | Apache-2.0   |
| `sketches-go/`   | github.com/DataDog/sketches-go, `ddsketch/pb`          | v1.4.8                               | Apache-2.0   |
| `pprof/`         | github.com/google/pprof, `proto`                       | v0.0.0-20260906184651-6331bc6350fe   | Apache-2.0   |

One edit, in `agent-payload/proto/metrics/agent_payload.proto`: the
`gogoproto` import and its four `(gogoproto.nullable) = false` options are
removed. They only steer the Go code generator and change nothing on the wire.

To update: copy the same paths from the newer release, repeat that edit, build.
