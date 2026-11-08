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
Caddy продолжает обслуживать TLS и проксировать `/api/*`, `/ws`, `/health` на PvP;
остальные пути идут в `blitz-web`. Проверить главную, assets и `/health` по HTTPS.
Для отката первой миграции вернуть сохранённый Caddyfile, проверить `caddy validate`
и перезагрузить Caddy. Прежняя статика сохраняется. Для дальнейших откатов образа
использовать `rollback blitz-web`.
