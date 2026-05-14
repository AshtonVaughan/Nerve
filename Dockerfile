# Headless Nerve daemon image. Useful for benchmark runs and the integration
# CI matrix; not intended for real desktop control (no display server).
FROM rust:1.81-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    libxdo-dev libxtst-dev libxcb1-dev libdbus-1-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY core ./core
RUN cd core && cargo build --release --workspace

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libxdo3 libxtst6 libxcb1 libdbus-1-3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/core/target/release/nerve /usr/local/bin/nerve
ENV NERVE_BIND=0.0.0.0:8765 NERVE_LOG_DIR=/var/lib/nerve/logs
EXPOSE 8765
ENTRYPOINT ["/usr/local/bin/nerve"]
CMD ["start"]
