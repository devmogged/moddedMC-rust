import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Fabric Loom needs Java 25 or newer: the cached JDK, then JAVA_HOME, then the `java` on PATH.
const MIN_JAVA = 25;
const cwd = fileURLToPath(new URL("../../companion/", import.meta.url));
const windows = process.platform === "win32";

const javaHome = (java) => {
  const out = spawnSync(java, ["-XshowSettings:properties", "-version"], { encoding: "utf8" });
  const text = `${out.stdout ?? ""}${out.stderr ?? ""}`;
  const home = text.match(/java\.home = (.+)/)?.[1]?.trim();
  const major = Number(text.match(/java\.specification\.version = (\d+)/)?.[1]);
  return home && major >= MIN_JAVA ? home : null;
};
const bin = (home) => join(home, "bin", windows ? "java.exe" : "java");
const candidates = [
  fileURLToPath(new URL("../../cache/jdk-25.0.5+8/", import.meta.url)),
  process.env.JAVA_HOME,
].filter((home) => home && existsSync(bin(home)));
const home = candidates.map((h) => javaHome(bin(h))).find(Boolean) ?? javaHome("java");
if (!home) {
  console.error(`The companion needs Java ${MIN_JAVA} or newer; set JAVA_HOME to one.`);
  process.exit(1);
}

const env = { ...process.env, JAVA_HOME: home };
const command = windows ? "cmd.exe" : "./gradlew";
const args = windows ? ["/d", "/c", ".\\gradlew.bat", "build"] : ["build"];
const result = spawnSync(command, args, { cwd, env, stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
