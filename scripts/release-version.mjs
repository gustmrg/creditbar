// Resolves, checks and applies a release version. Commands that compare against published tags
// read `git ls-remote --tags origin` on stdin.
//
//   current                      prints the version in crates/app/tauri.conf.json
//   resolve <current> <request>  prints the version <request> asks for: patch, minor, major,
//                                or an exact version (1.2.3, 1.3.0-beta.1)
//   next-beta <current>          prints the next minor version's next beta
//   check <version>              fails unless every install can update to <version>,
//                                and prints the tag its release notes start from
//   set <version>                writes <version> into tauri.conf.json, Cargo.toml and Cargo.lock
import { readFileSync, writeFileSync } from "node:fs";

const PATTERN = /^(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/;
const TAURI_CONF = "crates/app/tauri.conf.json";
const WORKSPACE_TOML = "Cargo.toml";
const LOCKFILE = "Cargo.lock";
const CRATES = ["creditbar", "creditbar-core"];

function parse(version) {
  const match = PATTERN.exec(version);
  if (!match) return null;
  // A release sorts above every beta of the same version.
  return [Number(match[1]), Number(match[2]), Number(match[3]), match[4] === undefined ? Infinity : Number(match[4])];
}

function compare(a, b) {
  const [x, y] = [parse(a), parse(b)];
  for (let i = 0; i < 4; i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return 0;
}

const isBeta = (version) => version.includes("-");
const base = (version) => version.split("-")[0];

function fail(message) {
  console.error(`::error::${message}`);
  process.exit(1);
}

function publishedTags() {
  return readFileSync(0, "utf8")
    .split("\n")
    .map((line) => /refs\/tags\/v(\S+)$/.exec(line.trim())?.[1])
    .filter((version) => version && parse(version))
    .sort(compare);
}

/** Replaces exactly one match of `pattern`, or fails naming `file`. */
function replaceOnce(text, pattern, replacement, file) {
  const matches = text.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`)) ?? [];
  if (matches.length !== 1) fail(`Expected one version to update in ${file}, found ${matches.length}.`);
  return text.replace(pattern, replacement);
}

const [command, ...args] = process.argv.slice(2);

if (command === "current") {
  console.log(JSON.parse(readFileSync(TAURI_CONF, "utf8")).version);
} else if (command === "resolve") {
  const [current, request] = args;
  if (!parse(current ?? "")) fail(`The current version ${current} is not a release or a beta.`);
  const [major, minor, patch] = parse(base(current));
  const next = {
    patch: `${major}.${minor}.${patch + 1}`,
    minor: `${major}.${minor + 1}.0`,
    major: `${major + 1}.0.0`,
  }[request];
  const version = next ?? request;
  if (!parse(version ?? "")) fail(`${request} is neither patch, minor, major, a release (1.2.3) nor a beta (1.3.0-beta.1).`);
  console.log(version);
} else if (command === "next-beta") {
  // 0.18.0 leads to 0.19.0-beta.1, then 0.19.0-beta.2, so every beta sorts above the latest
  // release.
  const published = publishedTags();
  const [major, minor] = parse(base(args[0]));
  const next = `${major}.${minor + 1}.0`;
  const last = Math.max(0, ...published.filter((tag) => base(tag) === next && isBeta(tag)).map((tag) => parse(tag)[3]));
  console.log(`${next}-beta.${last + 1}`);
} else if (command === "check") {
  const [version] = args;
  if (!parse(version ?? "")) fail(`${version} is neither a release (1.2.3) nor a beta (1.3.0-beta.1).`);
  const published = publishedTags();
  if (published.includes(version)) fail(`Tag v${version} already exists.`);
  // No install should ever be offered a lower version.
  const newest = published.at(-1);
  if (newest && compare(version, newest) < 0) {
    fail(
      isBeta(newest)
        ? `v${newest} is out as a beta, and v${version} sorts below it. Release ${base(newest)} or higher (version: minor).`
        : `v${newest} is already out, and v${version} is lower.`,
    );
  }
  // A beta's notes list what changed since the version before it; a release's list everything
  // since the previous release, betas included.
  const previous = published.filter((tag) => isBeta(version) || !isBeta(tag)).at(-1);
  console.log(previous ? `v${previous}` : "");
} else if (command === "set") {
  const [version] = args;
  if (!parse(version ?? "")) fail(`${version} is neither a release (1.2.3) nor a beta (1.3.0-beta.1).`);

  const conf = JSON.parse(readFileSync(TAURI_CONF, "utf8"));
  conf.version = version;
  writeFileSync(TAURI_CONF, `${JSON.stringify(conf, null, 2)}\n`);

  const toml = readFileSync(WORKSPACE_TOML, "utf8");
  writeFileSync(
    WORKSPACE_TOML,
    replaceOnce(toml, /(\[workspace\.package\][^[]*?\nversion = ")[^"]*(")/, `$1${version}$2`, WORKSPACE_TOML),
  );

  let lock = readFileSync(LOCKFILE, "utf8");
  for (const crate of CRATES) {
    lock = replaceOnce(lock, new RegExp(`(\\nname = "${crate}"\\nversion = ")[^"]*(")`), `$1${version}$2`, LOCKFILE);
  }
  writeFileSync(LOCKFILE, lock);
  console.log(`Set the version to ${version}.`);
} else {
  fail(`Unknown command ${command}. Use current, resolve, next-beta, check or set.`);
}
