# SA2 Chao Online dedicated server (sa2co-server): one static binary, nothing else.
#   docker build -t sa2-chao-online-server .
#   docker run -d -p 43902:43902/udp -v /path/to/skins:/skins:ro \
#     --name sa2-chao-online-server sa2-chao-online-server
# Skins in /skins (one folder per skin, as in the mod's skins/) go to every player who joins.
FROM rust:1.99.0-alpine3.22 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release --locked --target x86_64-unknown-linux-musl --bin sa2co-server \
    && strip target/x86_64-unknown-linux-musl/release/sa2co-server

FROM scratch
COPY --from=build /src/target/x86_64-unknown-linux-musl/release/sa2co-server /sa2co-server
USER 65534:65534
EXPOSE 43902/udp
ENV SA2CO_PORT=43902
ENV SA2CO_SKINS=/skins
LABEL org.opencontainers.image.title="SA2 Chao Online server" \
      org.opencontainers.image.description="Dedicated lobby server for the SA2 Chao Online mod" \
      org.opencontainers.image.source="https://github.com/Regrant-Dev/SA2-CHAO-ONLINE" \
      org.opencontainers.image.licenses="MIT"
ENTRYPOINT ["/sa2co-server"]
