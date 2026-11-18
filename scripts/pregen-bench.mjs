// Runs a server, waits until it is ready, sends the pregen commands and prints time and peak RSS
// as JSON. Used by scripts/pregen-bench.ps1.
// usage: node pregen-bench.mjs <cwd> <ready-regex> <done-regex> <commands;separated> -- <exe> [args...]
import { spawn, execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";

const [cwd, ready, done, commands, sep, exe, ...args] = process.argv.slice(2);
if (sep !== "--") throw new Error("usage: node pregen-bench.mjs <cwd> <ready> <done> <commands> -- <exe> [args...]");
const log = `${cwd}/bench.log`;
const child = spawn(exe, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
let peak = 0;
let started = 0;
const rss = () => {
  try {
    const out = execFileSync("tasklist", ["/FI", `PID eq ${child.pid}`, "/FO", "CSV", "/NH"]).toString();
    const bytes = Number(out.split('","')[4]?.replace(/[^\d]/g, "")) * 1024;
    if (bytes) peak = Math.max(peak, bytes);
    return bytes || 0;
  } catch {
    return 0;
  }
};
const sampler = setInterval(rss, 1000);
const ansi = new RegExp(String.fromCharCode(27) + "\\[[0-9;]*m", "g");
let phase = "boot";
let idle = 0;
const onLine = (line) => {
  appendFileSync(log, line + "\n");
  if (phase === "boot" && new RegExp(ready).test(line)) {
    phase = "run";
    idle = rss();
    started = Date.now();
    for (const c of commands.split(";")) child.stdin.write(c + "\n");
  } else if (phase === "run" && new RegExp(done).test(line)) {
    phase = "done";
    const secs = (Date.now() - started) / 1000;
    rss();
    const result = line.replace(ansi, "").replace(/^.*\[Chunky\] /, "");
    console.log(JSON.stringify({ secs, idle_mb: Math.round(idle / 2 ** 20), peak_mb: Math.round(peak / 2 ** 20), result }));
    child.stdin.write("stop\n");
    setTimeout(() => child.kill(), 60000).unref();
  }
};
let buf = "";
for (const stream of [child.stdout, child.stderr]) {
  stream.on("data", (d) => {
    buf += d.toString();
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      onLine(buf.slice(0, i).replace(/\r$/, ""));
      buf = buf.slice(i + 1);
    }
  });
}
child.on("exit", () => clearInterval(sampler));
