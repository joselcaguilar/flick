import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";

const detector =
  process.env.IMPECCABLE_DETECTOR ??
  "/Users/joselcaguilar/Library/Application Support/com.github.githubapp/app-skills/impeccable/scripts/detect.mjs";

if (existsSync(detector)) {
  const result = spawnSync(process.execPath, [detector, ...process.argv.slice(2)], { stdio: "inherit" });
  process.exit(result.status ?? 1);
}

const json = process.argv.includes("--json");
if (json) {
  process.stdout.write(
    `${JSON.stringify({ findings: [], fallback: "impeccable detector not installed on this host" })}\n`,
  );
} else {
  process.stdout.write("No detector binary found; fallback emitted no findings.\n");
}
