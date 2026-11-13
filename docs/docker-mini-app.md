# Telegram Mini App в Docker

GitHub Actions собирает Rust → WASM, проверяет TypeScript и frontend-тесты,
собирает Vite и проверяет nginx в read-only контейнере перед публикацией.
Образ: `ghcr.io/olegdurandin/blitz-web:sha-<7 символов коммита>`.
Публичные параметры VITE задаются build args; `.env` в образ не копируются.
PvP остаётся отдельным контейнером `blitz-pvp` на основном сервере.
VK-хостинг обновляется прежней командой из `web`.

После успешного CI, из соседнего bots-server:

```sh
./bots.sh 85.137.95.37 site blitz-tg sha-<коммит>
./bots.sh 85.137.95.37 check
./bots.sh 85.137.95.37 logs blitz-web
./bots.sh 85.137.95.37 rollback blitz-web
```

При первой миграции сохранить Caddyfile и `/opt/bots/sites/blitz-tg` в отдельную
датированную директорию `/opt/bots/backups/`. Сначала поднять образ и проверить
`http://127.0.0.1:8092/healthz`, затем выполнить `./bots.sh 85.137.95.37 caddy`.
Caddy продолжает обслуживать TLS и раздавать `blitz-web`. Клиент подключается
напрямую к каноническому HTTPS/WebSocket PvP; same-origin proxy опционален.
Проверить главную, assets и `/healthz` по HTTPS, а `/health` — на PvP-домене.
Для отката первой миграции вернуть сохранённый Caddyfile, проверить `caddy validate`
и перезагрузить Caddy. Прежняя статика сохраняется. Для дальнейших откатов образа
использовать `rollback blitz-web`.

## Маршрут API в релизе 3.3.1

При выкладке обнаружен timeout прямых соединений между VPS 85.137.95.37 и
62.109.4.100. DNS разрешался правильно, контейнеры healthy, проверенные host
firewall-правила не содержали запрета этого peer. Межсерверный маршрут требует
отдельной проверки у инфраструктуры; он не нужен для прямого клиентского API.

В production-сборке `VITE_PVP_PROXY_ORIGIN` оставлен пустым: обе Mini Apps
используют `VITE_PVP_URL=https://pvp.62-109-4-100.sslip.io`. Это существующий
канонический backend, не новый сервис. Проверенная авторизация, TLS и точный
CORS allowlist сохраняются. Старые Caddy proxy routes могут оставаться, но
новая клиентская сборка их не вызывает. Не проверять доступность backend через
Telegram `/health`; проверять прямой PvP `/health` и Telegram `/healthz`.
