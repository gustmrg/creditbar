// Writes SHA256SUMS.txt for the release artifacts in a directory (default: dist).
import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const directory = path.resolve(process.argv[2] ?? "dist");
const names = (await readdir(directory)).filter((name) => /\.(?:dmg|deb|AppImage)$/i.test(name)).sort();
const lines = [];
for (const name of names) {
  const digest = createHash("sha256")
    .update(await readFile(path.join(directory, name)))
    .digest("hex");
  lines.push(`${digest}  ${name}`);
}
if (!lines.length) throw new Error(`No release artifacts found in ${directory}.`);
await writeFile(path.join(directory, "SHA256SUMS.txt"), `${lines.join("\n")}\n`, "utf8");
console.log(`Checksummed ${lines.length} release artifacts.`);
