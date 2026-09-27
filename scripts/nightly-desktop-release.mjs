import { execFileSync, spawnSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export function isDesktopPackageInput(file) {
  if (/(^|\/)(?:tests?|__tests__|fixtures)\//.test(file) || /\.(?:test|spec)\.[^/]+$/.test(file)) return false;
  return /^(?:clients\/(?:desktop|web|extension)\/|crates\/|scripts\/|patches\/|\.cargo\/|\.github\/(?:workflows|scripts)\/|update-server\/|Cargo\.(?:toml|lock)$|rust-toolchain\.toml$|CHANGELOG\.md$)/.test(file);
}

export function nightlyIdentity(baseVersion, runNumber, attempt, sha) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(baseVersion);
  if (!match || !/^[0-9a-f]{40}$/.test(sha)) throw new Error("Exact numeric source version and SHA are required");
  if (!Number.isSafeInteger(runNumber) || runNumber < 1 || !Number.isSafeInteger(attempt) || attempt < 1 || attempt > 99) {
    throw new Error("Invalid nightly workflow run or attempt");
  }
  const major = Number(match[1]);
  const minor = Number(match[2]) + 1;
  const patch = runNumber * 100 + attempt;
  if (major > 255 || minor > 255 || patch > 65535) {
    throw new Error("Nightly version exceeds native package bounds; advance the release train and workflow sequence");
  }
  const version = `${major}.${minor}.${patch}`;
  return { version, tag: `desktop-latest-v${version}`, sha };
}

export function newestVerifiedMainCommit(runs, isAncestor) {
  const seen = new Set();
  return runs.find((run) => {
    if (seen.has(run.head_sha)) return false;
    seen.add(run.head_sha);
    return run.status === "completed" && run.conclusion === "success" && isAncestor(run.head_sha);
  });
}

function git(...args) {
  return execFileSync("git", args, { encoding: "utf8" }).trim();
}

function api(path, projection = ".") {
  return JSON.parse(execFileSync("gh", ["api", path, "--jq", projection], { encoding: "utf8" }));
}

function select() {
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo) throw new Error("GITHUB_REPOSITORY is required");
  const runs = api(`repos/${repo}/actions/workflows/ci.yml/runs?branch=main&event=push&per_page=100`, ".workflow_runs | map({head_sha,status,conclusion})");
  const candidate = newestVerifiedMainCommit(runs, (sha) => spawnSync("git", ["merge-base", "--is-ancestor", sha, "origin/main"]).status === 0);
  if (!candidate) throw new Error("No verified main commit in the latest 100 CI runs");
  const releases = [];
  for (let page = 1; ; page++) {
    const batch = api(`repos/${repo}/releases?per_page=100&page=${page}`, "map({tag_name,draft,prerelease})");
    releases.push(...batch);
    if (batch.length < 100) break;
  }
  const previous = releases.filter((release) => !release.draft && release.prerelease && /^desktop-latest-v\d+\.\d+\.\d+$/.test(release.tag_name))
    .sort((left, right) => {
      const a = left.tag_name.slice("desktop-latest-v".length).split(".").map(Number);
      const b = right.tag_name.slice("desktop-latest-v".length).split(".").map(Number);
      return b[0] - a[0] || b[1] - a[1] || b[2] - a[2];
    })[0];
  const sha = candidate.head_sha;
  if (previous && process.env.FORCE_BUILD !== "true") {
    const old = git("rev-parse", `${previous.tag_name}^{commit}`);
    if (spawnSync("git", ["merge-base", "--is-ancestor", sha, old]).status === 0) {
      appendFileSync(process.env.GITHUB_OUTPUT, "build=false\n");
      console.log("Skipping: Latest already contains the newest verified source");
      return;
    }
    if (spawnSync("git", ["merge-base", "--is-ancestor", old, sha]).status !== 0) {
      throw new Error("Latest release is not an ancestor of the verified source");
    }
    const changed = git("diff", "--name-only", old, sha).split("\n").filter(isDesktopPackageInput);
    if (!changed.length) {
      appendFileSync(process.env.GITHUB_OUTPUT, "build=false\n");
      console.log("Skipping: no packaged desktop inputs changed since Latest");
      return;
    }
  }
  const source = JSON.parse(git("show", `${sha}:clients/desktop/src-tauri/tauri.conf.json`));
  const identity = nightlyIdentity(source.version, Number(process.env.GITHUB_RUN_NUMBER), Number(process.env.GITHUB_RUN_ATTEMPT), sha);
  appendFileSync(process.env.GITHUB_OUTPUT, `build=true\nsha=${identity.sha}\nversion=${identity.version}\ntag=${identity.tag}\n`);
  console.log(JSON.stringify(identity));
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try { select(); } catch (error) { console.error(error); process.exitCode = 1; }
}
