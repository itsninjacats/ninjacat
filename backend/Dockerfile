# The production image of the Rust server: one image, every program in it. The command says which
# one a container runs, `ninjapaws_internal`, `ninjapaws_api` or another; there is no default.
#
#   docker build -t ninjapaws .
#   docker run ninjapaws ninjapaws_internal
#
# PROFILE is the Cargo profile: `dist` (the default) for an image that is deployed, `dev` for the
# dev stack, which rebuilds on every change and does not need the optimiser's time.
FROM rust:1.99.1-slim-trixie AS build
ARG PROFILE=dist
WORKDIR /src
COPY . .

# Cache mounts keep the registry and the build between builds, so a change rebuilds only what
# it touches. The target directory is a mount, so the programs are copied out in the same step,
# every `ninjapaws_*` of the build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --profile "$PROFILE" --locked -p api --bins \
    && mkdir /out \
    && find "target/$([ "$PROFILE" = dev ] && echo debug || echo "$PROFILE")" -maxdepth 1 -type f -executable -name 'ninjapaws_*' -exec cp {} /out/ \;

# cc: glibc and libgcc, all a Rust binary links against; nonroot: uid 65532.
FROM gcr.io/distroless/cc-debian13:nonroot

# The licences of the copied and generated code ask for their notices to travel with every
# copy, the image included.
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/ninjapaws/
COPY proto/protos /usr/share/doc/ninjapaws/protos

COPY --from=build /out/ /usr/local/bin/

# ninjapaws_internal listens on 8081, the intake hosts on 8080, ninjapaws_monitors' health on 8559.
EXPOSE 8080 8081 8559
