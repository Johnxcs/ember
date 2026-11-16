FROM rust:1-slim AS build
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/ember-server /usr/local/bin/ember-server
# Render sets PORT automatically; the server reads it.
CMD ["ember-server"]
