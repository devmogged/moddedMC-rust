---
name: moddedMC
description: Desktop workspace for hosting and joining modded Minecraft servers.
colors:
  ground: "oklch(0.187 0.027 258)"
  raised: "oklch(0.225 0.033 256)"
  surface: "oklch(0.249 0.038 256)"
  selected: "oklch(0.294 0.045 255)"
  well: "oklch(0.164 0.026 259)"
  hairline: "oklch(0.78 0.04 250 / 0.16)"
  hairline-strong: "oklch(0.79 0.055 250 / 0.29)"
  text: "oklch(0.96 0.013 250)"
  text-muted: "oklch(0.79 0.033 250)"
  text-faint: "oklch(0.71 0.035 253)"
  log-ink: "oklch(0.88 0.025 251)"
  brass: "oklch(0.85 0.125 83)"
  brass-hover: "oklch(0.9 0.11 83)"
  brass-ink: "oklch(0.225 0.048 62)"
  ok: "oklch(0.83 0.155 155)"
  warn: "oklch(0.83 0.15 63)"
  bad: "oklch(0.78 0.155 24)"
  starting: "oklch(0.82 0.12 227)"
  external: "oklch(0.81 0.105 302)"
  bar-off: "oklch(0.75 0.05 255 / 0.24)"
  scrim: "oklch(0.1 0.02 260 / 0.67)"
  scroll: "oklch(0.78 0.04 250 / 0.25)"
  scroll-hover: "oklch(0.81 0.04 250 / 0.4)"
typography:
  body:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, system-ui, sans-serif'
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
  title:
    fontSize: "clamp(24px, 2.5vw, 33px)"
    fontWeight: 650
    lineHeight: 1.16
    letterSpacing: "-0.035em"
  reading:
    fontSize: "clamp(19px, 2.35vw, 31px)"
    fontWeight: 650
    lineHeight: 1.15
    letterSpacing: "-0.035em"
  label:
    fontSize: "12px"
  mono:
    fontFamily: '"Cascadia Mono", ui-monospace, "SF Mono", Consolas, monospace'
    fontSize: "12px"
rounded:
  inset: "4px"
  control: "6px"
  chart: "9px"
  overlay: "10px"
spacing:
  tight: "6px"
  action: "8px"
  grid: "12px"
  gutter: "28px"
components:
  button:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "36px"
  button-primary:
    backgroundColor: "{colors.brass}"
    textColor: "{colors.brass-ink}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "36px"
  button-primary-hover:
    backgroundColor: "{colors.brass-hover}"
  input:
    backgroundColor: "{colors.ground}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 10px"
    height: "36px"
  chart:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.chart}"
    padding: "13px 15px 12px"
  console:
    backgroundColor: "{colors.well}"
    rounded: "{rounded.overlay}"
  palette:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.overlay}"
    width: "min(560px, calc(100vw - 48px))"
---

# Design System for moddedMC

## Overview

The server icon illuminates its header. Five health bars carry the familiar multiplayer-list read into a desktop workspace for hosts. Keep the device index in place so the host can find a server, check its readings and move straight to the tab they need.

Cool ink surfaces and slate rules carry the structure. Warm brass marks primary actions and keyboard focus. Pixel rendering belongs to the server icons.

## Colors

Color keys in the frontmatter match CSS custom properties in [the stylesheet](app/src/styles.css) with a `--` prefix.

| Role | Tokens and use |
| --- | --- |
| Actions | `brass`, `brass-hover` and `brass-ink` for primary controls. Brass also marks focus, the caret and text selection. |
| Health | `ok` for healthy, `warn` for degraded, and `bad` for errors, a crash or critically low TPS. `bar-off` holds unlit steps and meter tracks. |
| Activity | `starting` for lifecycle transitions, Players and CPU usage. `external` for external servers and the RAM channel. |
| Structure | `ground` for the sidebar and fields, `raised` for the workspace and controls, `surface` for headers and charts, `selected` for general selection, and `well` for logs and telemetry. Hairlines divide surfaces. |
| Ink | `text` for names and values, `text-muted` for labels, `text-faint` for timestamps and placeholders, and `log-ink` for the console. |

Use the server hue for identity. The default `--server-hue` is `255`. The current server sets it from the icon. `--server-accent` uses `oklch(0.82 0.1 var(--server-hue))` for the row marker and tab underline. `--server-tint` uses `oklch(0.59 0.16 var(--server-hue))` for the selected row and header wash.

## Typography

Use `--ui` for the interface. Pane titles and live readings share the 650 weight. Buttons, section headings and row names use 600. Keep labels at 12px and compact device readings at 10px. Tabular numerals keep changing data aligned.

Use `--mono` for logs, command input, paths and shortcut keys. Times recede, while log messages and severity remain easy to scan.

## Layout

The desktop shell uses `clamp(236px, 24vw, 274px)` for the sidebar and puts the remaining space into the workspace. Devices, Join and Packs scroll in the sidebar. Mods stays in its footer.

A managed server opens with a header of at least 106px, the equal-width TPS, MSPT, Players and RAM strip, then 44px tabs for Console, Performance, Mods, Backups and Pregen. The header and content use 28px horizontal gutters. Performance places a full-width tick chart above a two-column chart grid. Device, pack, catalog and friend panes share the same header and ruled rows.

## Elevation & Depth

Use tonal steps and 1px hairlines for most surfaces. A radial wash starts at the server icon and fades into `--surface`. The header icon has a small shadow of `0 6px 15px -6px var(--scrim)`. Dialogs and toast cards lift with `--shadow-pop` (`0 12px 32px -8px oklch(0 0 0 / 0.6)`). Modal dialogs sit above the scrim.

## Shapes

Use 6px corners for rows, buttons and fields, 9px for charts, and 10px for the console and overlays. Keep the icons square at 32px in the sidebar, 40px in regular rows and 64px in headers. Lucide button icons use a 15px box and a 1.75px stroke.

## Components

### Server row and health

Display a decoded PNG favicon from the server status ping when available. Sample its dominant chromatic hue to tint the header, selected row and tab indicator. A missing or invalid image uses a name-seeded 8×8 ore icon. A valid image without a usable hue keeps the name-seeded tint.

The five bars read tick health for hosted servers and ping for friends. Hosted tick budgets derive from the configured tick rate. Show five green under half the budget, four green from half, three amber from 80%, and two amber from 100%. A TPS reading below 75% of the tick rate takes precedence and lights one red bar.

| State | Treatment |
| --- | --- |
| Running or Degraded | Live bars, player count when known and a status word in the header. Running with missing health uses unlit bars. |
| Starting or Stopping | Cyan bar sequence and the named state. |
| Stopped | Muted status word with the bars removed. |
| Crashed | Single red bar and the Crashed label. |
| External | Violet indicators when tick health is unavailable. The pane shows process and address facts with lifecycle controls reserved for managed servers. |

### Telemetry and work surfaces

Keep the live strip on every managed server tab. Missing values render as a dash with an empty trace. Charts use observed samples and leave gaps for missing readings. Performance adds pointer and keyboard inspection of recorded samples. The console sits in a recessed well with alternating lines, severity ink and a command field. Static skeleton rows cover initial loads. Load failures show the error and a Retry action.

Device meters appear when an online device has machine info. CPU uses reported utilization or shows the thread count with the meter omitted. RAM usage needs a positive total. Disk text shows free space, with a usage meter only when the total is available.

Pregen uses a square grid with concentric guides and a radial fill scaled to the square root of completion, so its area represents reported progress. Read it as an aggregate footprint estimate based on the reported percentage. Keep the numeric percentage, chunk count, rate and ETA tied to reported values. Running uses green and Paused uses amber.

### Controls, palette and feedback

Use solid brass for the primary action and bordered raised buttons for secondary actions. Fields use ink backgrounds and strong hairlines. A 2px brass outline marks keyboard focus with a 2px offset on most controls. The console and palette use an inset ring or brass rule where the field meets its surrounding surface.

Open the command palette from the sidebar search control or Ctrl/Cmd+K. Arrow keys move through results, Enter runs an enabled command and Escape closes the dialog. The selected result has a brass edge. Unavailable actions keep their reason visible.

Keep active work visible in place, such as a build target and its output or a running Pregen job. Dismissible toasts report backup, deploy and Pregen completion as well as operation errors. A success toast lasts 5.5 seconds and an error lasts 9 seconds. Active Pregen can report completion after leaving its tab.

### Motion

`--ease-out` uses `cubic-bezier(0.22, 1, 0.36, 1)`. `--spring` uses `linear(0, 0.55 28%, 0.87 53%, 1.012 78%, 1)`.

| Interaction | Default | Reduced motion |
| --- | --- | --- |
| Hover and press | `--t-hover` 120ms for color and scale. Press scales to 0.97 over 70ms. | Immediate color, no press scale. |
| Pane, row and overlay entry | `--t-pane` 180ms ease-out. Pane and tab content fade up 4px. Sidebar rows stagger by 25ms, capped at 100ms. Dialogs and toasts fade up 6px with a small scale change. | 80ms linear fade for panes, dialogs and toasts. Sidebar rows appear immediately. |
| Health bars | 160ms transitions, 24ms per bar. Starting and Stopping cycle every 1.05s with 62ms per bar. A crash pulses once for 230ms. | Bars switch immediately. Starting and Stopping use steady cyan. |
| Tabs, plots and Pregen | `--t-settle` 220ms for the tab underline, chart paths and dots, and Pregen extent. | Changes are immediate. |
| Live readings and console | Changed readings roll in over 185ms. Console follow uses native smooth scroll after the first paint of log lines. | Reading changes and console follow are immediate. |

## Do's and Don'ts

- Keep the selected server's name, state and controls in the same place across tabs.
- Pair status color with words, bar counts or values. Preserve separate loading, unavailable and error states.
- Use short action labels. Keep operational details such as device route, free disk space and action availability where they affect a decision.
- Use original interface marks and keep Minecraft imagery confined to the server icon.
