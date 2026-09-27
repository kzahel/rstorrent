import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export function resolveDesktopReleaseInput({ event, ref, sha, sourceSha, version, tag }) {
  if (sourceSha || version || tag) {
    if (!/^[0-9a-f]{40}$/.test(sourceSha ?? "") || !/^\d+\.\d+\.\d+$/.test(version ?? "") || tag !== `desktop-latest-v${version}`) {
      throw new Error("Nightly release requires exact source SHA, numeric version, and matching Latest tag");
    }
    return { sourceSha, version, tag, channel: "latest", publish: true };
  }
  if (event === "push" && /^refs\/tags\/desktop-v\d+\.\d+\.\d+$/.test(ref)) {
    const stableTag = ref.slice("refs/tags/".length);
    return { sourceSha: sha, version: stableTag.slice("desktop-v".length), tag: stableTag, channel: "stable", publish: true };
  }
  if (event === "push" && ref?.startsWith("refs/tags/desktop-v")) {
    throw new Error("Invalid Stable desktop release tag");
  }
  return { sourceSha: sha, version: "", tag: "", channel: "stable", publish: false };
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    const result = resolveDesktopReleaseInput({
      event: process.env.GITHUB_EVENT_NAME,
      ref: process.env.GITHUB_REF,
      sha: process.env.GITHUB_SHA,
      sourceSha: process.env.INPUT_SOURCE_SHA,
      version: process.env.INPUT_RELEASE_VERSION,
      tag: process.env.INPUT_RELEASE_TAG,
    });
    appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(result).map(([key, value]) => `${key}=${value}\n`).join(""));
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}
