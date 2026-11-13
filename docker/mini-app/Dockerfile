FROM rust:1.95-bookworm AS wasm
WORKDIR /src
RUN rustup target add wasm32-unknown-unknown
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p poker-wasm --target wasm32-unknown-unknown

FROM node:22-bookworm-slim AS build
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
COPY --from=wasm /src/target/wasm32-unknown-unknown/release/poker_wasm.wasm ./src/engine/generated/poker.wasm
ARG VITE_BUILD=dev
ARG VITE_PVP_URL=https://pvp.62-109-4-100.sslip.io
ARG VITE_PVP_PROXY_ORIGIN=
ARG VITE_TG_BOT=blitzpokerbot
RUN npm run typecheck && npx vitest run && npx vite build

FROM nginx:stable-alpine
COPY docker/mini-app/nginx.conf /etc/nginx/nginx.conf
COPY --from=build /src/web/dist/ /usr/share/nginx/html/
USER nginx
EXPOSE 8080
ENTRYPOINT ["nginx"]
CMD ["-g", "daemon off;"]
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1
