import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const version = "v2.0.2";
const digest = "6e62da9f56a3bcf72e0c62c29e94609c847cc9b110c460f68b20035525985238";
const vendor = new URL("../src-tauri/vendor/", import.meta.url);
const executable = new URL("playitd.exe", vendor);
const license = new URL("playit-LICENSE.txt", vendor);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

async function download(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(90_000) });
  if (!response.ok) throw new Error("Could not prepare Playit: HTTP " + response.status);
  return Buffer.from(await response.arrayBuffer());
}

await mkdir(vendor, { recursive: true });
const cached = await readFile(executable).catch((error) => {
  if (error.code !== "ENOENT") throw error;
  return Buffer.alloc(0);
});

if (sha256(cached) !== digest) {
  const bytes = await download(
    "https://github.com/playit-cloud/playit-agent/releases/download/" +
      version +
      "/playitd-windows-x86_64-signed.exe",
  );
  if (sha256(bytes) !== digest) throw new Error("Playit download checksum mismatch.");
  await writeFile(executable, bytes);
}

const notice = await download(
  "https://raw.githubusercontent.com/playit-cloud/playit-agent/" + version + "/LICENSE.txt",
);
await writeFile(license, notice);
