# App revamp

The owner wants the desktop app (`app/`, backlog A2 and A4) rebuilt: T3 Code's look and its
quality-of-life touches, a launch animation with an update bar, first-run install of the server
files, and four tabs. It stays on Tauri 2 + React (light: the system WebView2, about 10 MB) and
keeps every Rust command the app already has; only the screens are replaced.

## Source

T3 Code (`pingdotgg/t3code`, MIT) for the UI kit and patterns: `apps/web/src/components/ui`
(Base UI + Tailwind 4 + `class-variance-authority`), the theme tokens in `apps/web/src/index.css`,
toasts, tooltips, the command palette and the update flow (`apps/desktop/src/updates`). Copied
files keep T3 Code's MIT notice in `app/src/components/ui/LICENSE-T3CODE`. Electron-only parts
become their Tauri plugin equivalents (`updater`, `autostart`, `window-state`, `notification`).

## Launch

1. A splash with the new logo animating in (skipped with reduced motion).
2. A bar along the bottom: check for an app update (Tauri updater against GitHub Releases on
   `devmogged/moddedMC-rust`, signed), then check the server files.
3. First run only: install the required files (the agent and the server binary from the same
   release) with progress. Later releases update them in the background.
4. Start the local agent if it isn't running, then fade into the app.

## Tabs

| Tab | What it holds |
|---|---|
| Servers | Every server on every device, start/stop, console, stats, backups, pregen, its mods. New server: a guided setup (name, port, Java and Bedrock, online mode, players, MOTD, icon) that ends with how players reach it: same network, playit.gg, Tailscale or port forwarding, each walked through. Settings edit `pumpkin.toml` through the agent and save once. |
| Mods | The `.mmc` mods we've ported, with install to a server; drag and drop `.mmc` or Fabric `.jar` files. Packs live here too. |
| Jar → Rust | Coming soon. |
| Settings | Theme (system, light, dark), reduced motion, run on startup, start minimized to the tray, close to tray, update channel and "check now", devices (other machines with the agent), friends. |

## Stages

Each stage is one or more commits on `main`, checked with `npm run check` and `npm run build`.

1. Foundation: Tailwind 4, the T3 Code UI kit and tokens, the tab shell, the Settings tab. Done: the old
   screens run inside the Servers and Mods tabs (scoped by `legacy.css`) until stages 4 and 5
   replace them; DESIGN.md describes those old screens until then.
2. Brand: Pumpkin's logo with a MODDED banner (`app/app-icon.png`). Done for the app icon.
3. Launch: splash, updater, first-run install, local agent start.
4. Servers: list, detail views reskinned, the new-server setup with hosting, the settings editor
   (agent: read and write `pumpkin.toml` and the server icon).
5. Mods: catalogue, install, drag and drop, packs.
6. Releases: a workflow that builds the installer, the agent and the server, signs the update
   and publishes `latest.json`.
