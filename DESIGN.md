# Design

The app's visual world: **the multiplayer list, refined.** Every Minecraft player knows the server list: a row per server, a square icon, the name, a line of description, ping bars and a player count on the right. The app grows from that row into a dense, calm hosting tool at the craft level of T3 Code and Plaid. Mode: Operate. Precision and restraint, with patterns people already know.

## The row is the unit

- A server is always shown as a **row**: a 40px square server icon (the server's own `server-icon.png` when it has one, drawn with `image-rendering: pixelated`; otherwise a generated block-colored square from the server name), the name, one muted description line (pack and device), and a right-hand status cluster.
- The **status cluster** is the list's ping bars and player count, reinterpreted for hosts. Five bars show tick health: all five are lit at 20 TPS with MSPT under 25 ms, and fewer stay lit as MSPT climbs. Then the player count `3/20`. A stopped server shows no bars, a starting one shows bars filling in sequence, and a crashed one shows a single red bar and the word Crashed.
- The selected server's detail pane opens with the same row, larger (64px icon), so the list and the detail read as one object.
- Pixels appear in exactly one place: server icons. Nowhere else gets pixel fonts, block textures or dirt backgrounds.

## Layout

- A sidebar on the left (T3 Code style), 260px. It has the **Devices** groups (one per machine, with a connection dot and route label: This PC, Tailscale, SSH), each holding its server rows, then **Join** (servers you play on, as a friend), then **Mods**.
- The main pane is the selected item. For a server: the row header, then a quiet tab strip (Console, Performance, Mods, Backups), then the content.
- The window has no outer chrome beyond the system title bar. Content edges carry 1px hairlines, not boxes.

## Color

Dark by use scene: hosts check servers in the evening, next to the game. Everything is in OKLCH.

| Token | Value | Use |
| --- | --- | --- |
| `--ground` | `oklch(0.165 0.006 70)` | window background: near-black, a breath warm |
| `--raised` | `oklch(0.195 0.006 70)` | sidebar, hover rows |
| `--selected` | `oklch(0.235 0.008 70)` | selected row |
| `--hairline` | `oklch(1 0 0 / 0.075)` | every divider and outline |
| `--text` | `oklch(0.94 0.004 80)` | primary text |
| `--text-muted` | `oklch(0.70 0.006 80)` | description lines, labels (≥4.5:1 on ground) |
| `--text-faint` | `oklch(0.56 0.006 80)` | timestamps, placeholders (large or non-essential only) |
| `--brass` | `oklch(0.80 0.125 82)` | **the one accent**: primary action, focus ring, selection mark. Create's brass. |
| `--ok` | `oklch(0.77 0.16 148)` | lit health bars (the list's green) |
| `--warn` | `oklch(0.80 0.125 82)` | degraded bars; same hue as brass by design |
| `--bad` | `oklch(0.66 0.19 27)` | crashed, errors |

Brass appears only where something can be done or is selected. Status colors appear only in the status cluster, console severity and charts.

## Type

- UI uses the system stack, like T3 Code: `"Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, system-ui, sans-serif`. Base size 13px with 1.45 leading, and 12px for description lines. Weights are 400 and 600 only.
- Numbers in the UI use `font-variant-numeric: tabular-nums`, so player counts and MSPT never jitter.
- Monospace (`"Cascadia Mono", ui-monospace, "SF Mono", Consolas, monospace`, 12px) only where it is data: the console and command input.
- Headings are just larger weight-600 text (15px for pane titles). No display face. No eyebrows or kickers.

## Shape and depth

- Radius: 6px on controls and rows, 10px on popovers and dialogs. No pills except the tiny state dot.
- Elevation: hairlines for structure. Only popovers and dialogs get a shadow (`0 12px 32px -8px oklch(0 0 0 / 0.6)`), and never a hairline plus a wide shadow on the same element.
- Icons: lucide, 16px, 1.75 stroke, `--text-muted` at rest.

## Motion

- The authored moment is the **health bars**. When a server comes up they light one by one (bottom to top, 45 ms apart, 160 ms ease-out each), and when MSPT changes they settle to the new count. Nothing else on screen is choreographed.
- Everything else is functional: 120 ms ease-out for hover and selection, 180 ms for pane changes (opacity plus 4px of travel), with the console following new lines without animation.
- `prefers-reduced-motion`: bars switch instantly, and pane changes become crossfades of 80 ms or less.

## Browser surfaces

- Text selection: `--brass` at 28% alpha behind `--text`. The caret is `--brass`.
- Scrollbars: 8px thumbs in `oklch(1 0 0 / 0.12)` that brighten on hover, with no track.
- Focus: a 2px `--brass` outline at 2px offset on keyboard focus only (`:focus-visible`).

## Copy

Controls name their action (Start, Stop, Restart, Add device). Errors say what failed and what to do: "Can't reach the Pi over Tailscale. Is it on and logged in?" No taglines, helper text or secondary labels to explain a screen.
