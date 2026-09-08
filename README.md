<p align="center">
  <img src="docs/brand/caramba-cover.png" alt="Caramba — a ship sailing toward open water" width="100%">
</p>

<h1 align="center">Caramba</h1>
<p align="center"><strong>Your connection. Your course.</strong><br>VPN apps and a self-hosted platform for running your own service.</p>
<p align="center">
  <a href="https://github.com/semanticparadox/caramba/releases">Download apps</a> ·
  <a href="docs/DEPLOYMENT.md">Set up a server</a> ·
  <a href="docs/README.md">Documentation</a>
</p>

## One service, from server to screen

| | |
| :--- | :--- |
| **Caramba Connect** | A dedicated VPN app for connecting with your provider's subscription. |
| **Telegram Mini App** | Manage your subscription, devices, plans and support inside Telegram. |
| **Admin panel** | Manage servers, users, payments and your service's branding in one place. |
| **Self-hosted infrastructure** | Run the panel, bot, subscription service and node agents on your own servers. |

## Get connected

Open [Releases](https://github.com/semanticparadox/caramba/releases) and choose a **Caramba Connect** release for Android, macOS, Windows or Linux. Read its installation notes, then import the subscription supplied by your VPN provider.

Caramba is in beta. Platform capabilities and signing differ; use the release notes for the build you download. There is no public iOS download yet.

## Run your own service

Start with the [deployment guide](docs/DEPLOYMENT.md) for server requirements and configuration. On your server, launch the installer:

```bash
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

Choose a compact installation or distribute services across several hosts. The installer handles installation and upgrades; the panel handles day-to-day administration.

The server platform uses [sing-box](https://sing-box.sagernet.org/), with VLESS Reality, Hysteria2, TUIC and other supported transports. See the [protocol guide](docs/protocols.md) for configuration details.

## Explore

- [Install and operate](docs/DEPLOYMENT.md) — deployment, upgrades and backups.
- [Configure your service](docs/CONFIGURATION.md) — runtime settings.
- [Documentation](docs/README.md) — user, operator and developer guides.
- [Develop Caramba](docs/DEVELOPMENT.md) — build, test and understand the source.

## License

No project license has been published yet. See the repository for available licensing information.
