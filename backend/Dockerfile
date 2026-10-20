# The production image of the Rust server: one image, every program in it. The command says which
# one a container runs, `ninjapaws_internal` or another; there is no default.
#
#   docker build -t ninjapaws .
#   docker run ninjapaws ninjapaws_internal
FROM rust:1.99.1-slim-trixie AS build
WORKDIR /src
COPY . .

# Cache mounts keep the registry and the build between builds, so a change rebuilds only what
# it touches. The target directory is a mount, so the binaries are copied out in the same step;
# a program added to `api` is added to the list here.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p api --bins \
    && mkdir /out \
    && cp target/release/ninjapaws_api target/release/ninjapaws_app target/release/ninjapaws_internal \
       target/release/ninjapaws_monitors /out/

# cc: glibc and libgcc, all a Rust binary links against; nonroot: uid 65532.
FROM gcr.io/distroless/cc-debian13:nonroot

# The licences of the copied and generated code ask for their notices to travel with every
# copy, the image included.
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/ninjapaws/
COPY proto/protos /usr/share/doc/ninjapaws/protos

COPY --from=build /out/ /usr/local/bin/

# ninjapaws_internal listens on 8081, ninjapaws_monitors' health on 8559.
EXPOSE 8081 8559
