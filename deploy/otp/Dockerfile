# `otp` (publisher and CLI patcher) as a container, for publishing on the server that holds the master client:
#   docker build -f deploy/otp/Dockerfile -t openthor-otp .
#   docker run --rm -v /srv/master:/master:ro -v /srv/site:/site -v /srv/otp:/cfg openthor-otp publish --project /cfg/otp-project.json
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# the Tauri apps are not needed here: build only the CLI, drop them from the workspace
RUN sed -i 's#, "apps/maker/src-tauri", "apps/patcher/src-tauri"##' Cargo.toml && cargo build --release -p otp-cli -p otp-mcp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/otp /src/target/release/otp-mcp /usr/local/bin/
ENTRYPOINT ["otp"]
