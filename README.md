# GitHub Release Telegram Bot

A Telegram bot to be notified of new releases from GitHub repositories you care about.

Run it with Docker:

```sh
docker run -d \
  --name github-release-bot \
  --env-file .env \
  -e ENVIRONMENT_FILE=false \
  --restart unless-stopped \
  ghcr.io/pgoodjohn/github-releases-telegram-bot:latest
```

Or with docker compose:

```yaml
version: "3.9"

services:
  bot:
    image: ghcr.io/pgoodjohn/github-releases-telegram-bot:latest
    container_name: github-release-bot
    environment:
      TELOXIDE_TOKEN: "YOUR_TELEGRAM_TOKEN"
      GITHUB_TOKEN: "YOUR_GITHUB_TOKEN"
      ENVIRONMENT_FILE: "false"
    restart: unless-stopped
```

You can use the `DATABASE_PATH` environment variable to specify the location of the sqlite database.