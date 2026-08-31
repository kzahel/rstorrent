#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { parse } from "@babel/parser";
import { fileURLToPath } from "node:url";

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = path.join(webRoot, "src");
const catalogPath = path.join(sourceRoot, "localization/messages/en.json");
const catalog = JSON.parse(fs.readFileSync(catalogPath, "utf8"));
const attributeNames = new Set([
  "alt", "aria-description", "aria-label", "data-label", "emptyMessage",
  "label", "placeholder", "title",
]);
const propertyNames = new Set([
  "caption", "description", "disabledReason", "emptyMessage", "eyebrow",
  "heading", "help", "label", "message", "note", "placeholder", "reason",
  "title",
]);
const roots = ["inspection/components", "remote"];
const explicitFiles = [
  "inspection/bootstrap.tsx",
  "inspection/companion-bootstrap.tsx",
  "inspection/demo/catalog.ts",
  "inspection/demo/DemoApplication.ts",
  "inspection/file-actions.ts",
  "inspection/format.ts",
  "inspection/live/LiveApplication.ts",
  "inspection/peerFlags.ts",
  "inspection/tabs.ts",
  "inspection/torrent-actions.ts",
  "inspection/torrentFile.ts",
  "inspection/torrentInput.ts",
  "android-companion-client.ts",
];

for (const relativePath of [...walkRoots(), ...explicitFiles]) migrate(relativePath);
fs.writeFileSync(catalogPath, `${JSON.stringify(sortObject(catalog), null, 2)}\n`);

function* walkRoots() {
  for (const root of roots) {
    const pending = [path.join(sourceRoot, root)];
    while (pending.length > 0) {
      const current = pending.pop();
      for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
        const absolute = path.join(current, entry.name);
        if (entry.isDirectory()) pending.push(absolute);
        if (
          entry.isFile() &&
          /\.tsx?$/.test(entry.name) &&
          !/\.test\.tsx?$/.test(entry.name)
        ) {
          yield path.relative(sourceRoot, absolute);
        }
      }
    }
  }
}

function migrate(relativePath) {
  const absolutePath = path.join(sourceRoot, relativePath);
  const source = fs.readFileSync(absolutePath, "utf8");
  const sourceFile = parse(source, {
    sourceType: "module",
    plugins: ["typescript", "jsx"],
  });
  const edits = [];
  const filePropertyNames = new Set(propertyNames);
  if (relativePath === "inspection/demo/catalog.ts") {
    filePropertyNames.delete("message");
    filePropertyNames.add("progressReason");
  }
  const namespace = relativePath
    .replace(/\.tsx?$/, "")
    .replace(/[^A-Za-z0-9]+/g, ".")
    .replace(/([a-z0-9])([A-Z])/g, "$1.$2")
    .toLowerCase();

  visit(sourceFile, []);
  if (edits.length === 0) return;
  edits.sort((left, right) => right.start - left.start);
  let output = source;
  for (const edit of edits) output = output.slice(0, edit.start) + edit.text + output.slice(edit.end);
  const importPath = path.relative(path.dirname(absolutePath), path.join(sourceRoot, "localization/runtime"))
    .replaceAll(path.sep, "/");
  const specifier = importPath.startsWith(".") ? importPath : `./${importPath}`;
  if (!output.includes('from "' + specifier + '"')) {
    output = `import { message as localizedMessage } from "${specifier}";\n${output}`;
  }
  fs.writeFileSync(absolutePath, output);

  function visit(node, ancestors) {
    if (node.type === "JSXText") {
      const normalized = normalize(node.value);
      if (isProductCopy(normalized)) {
        add(node.start, node.end, normalized, "jsx", jsxWhitespace(node.value));
      }
    } else if (
      node.type === "JSXAttribute" &&
      node.name.type === "JSXIdentifier" &&
      attributeNames.has(node.name.name) &&
      node.value?.type === "StringLiteral"
    ) {
      const value = normalize(node.value.value);
      if (isProductCopy(value)) add(node.value.start, node.value.end, value, "jsx");
    } else if (
      node.type === "ObjectProperty" &&
      filePropertyNames.has(propertyName(node.key)) &&
      node.value.type === "StringLiteral"
    ) {
      const value = normalize(node.value.value);
      if (isProductCopy(value)) add(node.value.start, node.value.end, value, "expression");
    } else if (
      node.type === "StringLiteral" &&
      isVisibleExpressionResult(node, ancestors) &&
      !isModuleSpecifier(node, ancestors.at(-1)) &&
      !isAlreadyHandled(ancestors.at(-1))
    ) {
      const value = normalize(node.value);
      if (isProductCopy(value)) add(node.start, node.end, value, "expression");
    } else if (
      node.type === "StringLiteral" &&
      isReturnValue(node, ancestors) &&
      /^[A-Z]/.test(normalize(node.value)) &&
      isProductCopy(normalize(node.value))
    ) {
      add(node.start, node.end, normalize(node.value), "expression");
    } else if (
      node.type === "StringLiteral" &&
      isPresentationCallArgument(node, ancestors.at(-1)) &&
      isProductCopy(normalize(node.value))
    ) {
      add(node.start, node.end, normalize(node.value), "expression");
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) {
        for (const child of value) {
          if (child && typeof child === "object" && typeof child.type === "string") {
            visit(child, [...ancestors, node]);
          }
        }
      } else if (value && typeof value === "object" && typeof value.type === "string") {
        visit(value, [...ancestors, node]);
      }
    }
  }

  function add(start, end, english, mode, whitespace = { leading: false, trailing: false }) {
    const base = `${namespace}.${slug(english)}`;
    let id = base;
    if (catalog[id]?.defaultMessage !== undefined && catalog[id].defaultMessage !== english) {
      id = `${base}.${crypto.createHash("sha256").update(english).digest("hex").slice(0, 7)}`;
    }
    catalog[id] = {
      defaultMessage: english,
      description: `Product copy in ${relativePath}`,
    };
    const call = `localizedMessage(${JSON.stringify(id)})`;
    const prefix = whitespace.leading ? '{" "}' : "";
    const suffix = whitespace.trailing ? '{" "}' : "";
    edits.push({ start, end, text: mode === "jsx" ? `${prefix}{${call}}${suffix}` : call });
  }
}

function isVisibleExpressionResult(node, ancestors) {
  let child = node;
  for (let index = ancestors.length - 1; index >= 0; index -= 1) {
    const current = ancestors[index];
    if (current.type === "JSXExpressionContainer") {
      if (current.expression !== child) return false;
      const containerParent = ancestors[index - 1];
      if (
        containerParent?.type === "JSXAttribute" &&
        (containerParent.name.type !== "JSXIdentifier" ||
          !attributeNames.has(containerParent.name.name))
      ) {
        return false;
      }
      return true;
    }
    if (current.type === "ConditionalExpression") {
      if (current.consequent !== child && current.alternate !== child) return false;
    } else if (current.type === "LogicalExpression") {
      if (current.operator !== "??" || current.right !== child) return false;
    } else if (current.type === "TemplateLiteral") {
      if (!current.expressions.includes(child)) return false;
    } else if (current.type !== "ParenthesizedExpression") {
      return false;
    }
    child = current;
  }
  return false;
}

function isReturnValue(node, ancestors) {
  const parent = ancestors.at(-1);
  if (parent?.type === "ReturnStatement" && parent.argument === node) return true;
  return parent?.type === "ArrowFunctionExpression" && parent.body === node;
}

function isPresentationCallArgument(node, parent) {
  if (parent?.type !== "CallExpression" || parent.arguments[0] !== node) return false;
  if (parent.callee.type !== "Identifier") return false;
  return parent.callee.name === "status" ||
    /^set(?:AcceptedMessage|Error|Feedback|FileActionStatus|Message|Notice|Status|Warning)$/.test(
      parent.callee.name,
    );
}

function isAlreadyHandled(parent) {
  return parent?.type === "JSXAttribute" || parent?.type === "ObjectProperty" ||
    parent?.type === "TSLiteralType";
}

function isModuleSpecifier(node, parent) {
  return (parent?.type === "ImportDeclaration" || parent?.type === "ExportNamedDeclaration" ||
    parent?.type === "ExportAllDeclaration") && parent.source === node;
}

function propertyName(node) {
  return node?.type === "Identifier" ? node.name : node?.type === "StringLiteral" ? node.value : "";
}

function normalize(value) {
  return value
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#(?:39|x27);/gi, "'")
    .replace(/\s+/g, " ")
    .trim();
}

function jsxWhitespace(value) {
  const first = value.search(/\S/);
  const last = value.search(/\S(?=\s*$)/);
  const before = first < 0 ? "" : value.slice(0, first);
  const after = last < 0 ? "" : value.slice(last + 1);
  return {
    leading: before.length > 0 && !/[\r\n]/.test(before),
    trailing: after.length > 0 && !/[\r\n]/.test(after),
  };
}

function isProductCopy(value) {
  if (value.length < 2 || !/[A-Za-z]/.test(value)) return false;
  if (/^(https?:|wss?:|[a-z]+_[a-z_]+$)/.test(value)) return false;
  if (/^[.#/][^\s]+$/.test(value)) return false;
  return true;
}

function slug(value) {
  const normalized = value.normalize("NFKD").replace(/[\u0300-\u036f]/g, "")
    .toLowerCase().replace(/[^a-z0-9]+/g, ".").replace(/^\.|\.$/g, "");
  const parts = normalized.split(".").filter(Boolean).slice(0, 7);
  return parts.join(".").slice(0, 64) || "message";
}

function sortObject(value) {
  return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)));
}
