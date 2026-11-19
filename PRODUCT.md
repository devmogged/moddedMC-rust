# Product

The desktop app for moddedMC-rust: one place to host modded Minecraft servers on your own machines and to join friends' servers. (The product name is not decided yet. "moddedMC" is a working name.)

## Platform

web

## Stack

Tauri 2 (Rust backend) with a React and TypeScript frontend built by Vite. This matches Plaid's launcher. The owner chose it so the backend stays Rust and the UI can reach T3 Code-level quality. The app lives in `app/`, a workspace of its own so the root workspace's CI is not affected.

## Users

- **Hosts.** Anyone who downloads the app to run servers for themselves and their friends: often one person with a laptop and a Raspberry Pi on Tailscale, or a box on the LAN. Their job is to pick mods, build a pack, put it on a machine, keep it running, and see when something is wrong (lag, RAM, a crash) without opening a terminal.
- **Players (friend mode).** Friends of a host who want to join that host's server. Their job is to get from "here's my server" to playing, with the right companion mod and assets, without touching a mods folder. This replaces the separate join exe.

A server's controls belong to whoever set it up. Players see a server to join, never its admin controls.

## Positioning

Rust servers and Rust mod ports, so packs that need 10 GB of RAM on Java run on a Raspberry Pi. Every machine you host on (this computer, a Pi over Tailscale, a LAN box over SSH) is managed the same way from one app, with no tokens or port forwarding. The app is the whole loop: browse ported mods, build a pack, deploy it, watch it, and send friends a way in.

## Operating Context

- Hosts reach their machines in three ways: the local machine, Tailscale (the device agent trusts the owner's Tailscale login), or SSH (tunneled to the agent). See `docs/DEVICES.md` for the agent API the app uses.
- Pack servers are built on the host's fast machine and pushed to slower devices. The Pi never compiles anything. Building can take minutes the first time and seconds after.
- Mods ship as drop-in `.mmc` files from the devmogged mod repos (public `mods`, private `mods-private`).
- Players are often on weak laptops and slow connections. Assets download once per pack version.
- Servers run in offline mode (no Mojang account checks). Skins are default, from Mojang's public API, or the player's choice.

## Capabilities and Constraints

- Devices: add, see their status (online, CPU, RAM, disk), install the agent over SSH.
- Servers per device: create, start, stop, restart, live state, console with command input, stats (MSPT, TPS, RAM, CPU, players), backups, pregeneration. Find servers already installed or running on a device.
- Mods and packs: browse ported mods, compose a pack, build it for a target (`x86_64` Windows or `aarch64` Linux), deploy it to a server. Adding or removing a mod mid-world is a rebuild plus a restart, after an automatic snapshot.
- Friend mode: join a friend's server. It installs the companion profile into whatever launcher the player has (or offers a zip) and adds the server to the server list.
- Undecided: the product name, the icon, whether friend mode is a separate install or a mode in the same app.

## Brand Commitments

- The feel is a quiet precision tool, inspired by T3 Code and Plaid (github.com/sting-raider/Plaid): dark, dense, calm and fast, with one warm accent. Minecraft shows up in small exact details, not pixel-art decoration. Inspired by, never a copy.
- Don't use Mojang or Minecraft logos, or anything that suggests the app is official.
- Motion is purposeful and smooth, and respects `prefers-reduced-motion`.
- No taglines, helper text or secondary labels to explain a screen. If a screen needs them, fix the layout.

## Product Principles

1. **One mental model for every machine.** Local, LAN and Pi servers look and behave the same.
2. **Show the server's health at a glance.** Lag, memory and crashes are visible before anyone complains.
3. **Never lose a world.** Destructive actions snapshot first and say what they will do.
4. **Fast is a feature.** The app opens instantly, updates live, and never blocks on a long build.
5. **The host is in charge.** Admin control stays with the server's owner. Players get a clean path to play and nothing more.
