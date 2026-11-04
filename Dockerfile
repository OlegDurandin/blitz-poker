# Образ сервера дуэлей Blitz Poker (crates/pvp-server). Собирается в GitHub Actions
# и публикуется в ghcr.io/olegdurandin/blitz-pvp; на сервере только скачивается
# (bots-server, bots/blitz-pvp). Клиент Mini App сюда не входит: он на хостинге VK.
#
# Зависимости — отдельным слоем (cargo-chef): правка своего кода не пересобирает их.

FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS plan
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=plan /src/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked -p pvp-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home bot \
    && mkdir /data && chown bot:bot /data
COPY --from=build /src/target/release/pvp-server /usr/local/bin/pvp-server

# Всё состояние — в /data: комнаты и счёт встреч в pvp.db (bots-backup копирует
# data/*.db). Внутри контейнера сервер слушает все адреса; наружу его выставляет
# только compose на 127.0.0.1, а HTTPS — Caddy хоста.
ENV PVP_BIND=0.0.0.0:8787 \
    PVP_DB=/data/pvp.db \
    RUST_LOG=pvp_server=info
USER bot
WORKDIR /data
VOLUME /data
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["pvp-server", "healthcheck"]
CMD ["pvp-server"]
