# gameshelf server image (research D14). Build from the repository root:
#   docker build -f docker/Dockerfile -t gameshelf .

FROM rust:1-bookworm AS chef
RUN rustup target add wasm32-unknown-unknown \
 && cargo install --locked cargo-chef cargo-leptos
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
# Dependencies of the server, then of the browser bundle (cargo-leptos builds it in target/front).
RUN cargo chef cook --release --recipe-path recipe.json -p gameshelf  && cargo chef cook --profile wasm-release --target wasm32-unknown-unknown --target-dir target/front       --recipe-path recipe.json -p gameshelf-frontend
COPY . .
RUN cargo leptos build --release

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /src/target/release/gameshelf /app/gameshelf
COPY --from=builder /src/target/site /app/site
ENV LEPTOS_OUTPUT_NAME=gameshelf \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:8080 \
    LEPTOS_RELOAD_PORT=3001 \
    LEPTOS_ENV=PROD \
    GAMESHELF_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080
USER nonroot
ENTRYPOINT ["/app/gameshelf"]
