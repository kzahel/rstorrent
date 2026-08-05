import { readdir, readFile } from "node:fs/promises";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const distributionRoot = resolve(scriptDirectory, "../dist");
const forbiddenPatterns = [
  ["Function constructor", /\b(?:new\s+)?Function\s*\(/],
  ["direct eval", /\beval\s*\(/],
  ["CommonJS require", /\brequire\s*\(/],
];

export function dynamicCodeViolations(source) {
  return forbiddenPatterns
    .filter(([, pattern]) => pattern.test(source))
    .map(([label]) => label);
}

async function javascriptFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) return javascriptFiles(path);
      return extname(entry.name) === ".js" ? [path] : [];
    }),
  );
  return nested.flat();
}

async function checkDistribution() {
  const files = await javascriptFiles(distributionRoot);
  if (files.length === 0) {
    throw new Error(`no JavaScript bundles found below ${distributionRoot}`);
  }

  const violations = [];
  for (const path of files) {
    const source = await readFile(path, "utf8");
    for (const label of dynamicCodeViolations(source)) {
      violations.push(`${path}: ${label}`);
    }
  }

  if (violations.length > 0) {
    throw new Error(
      `production bundle contains browser-unsafe code:\n${violations.join("\n")}`,
    );
  }

  console.log(
    `Browser bundle check passed: ${files.length} JavaScript bundles use no eval, Function constructor, or CommonJS require.`,
  );
}

if (
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await checkDistribution();
}
