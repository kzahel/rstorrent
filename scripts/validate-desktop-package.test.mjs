import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  validateLinuxDesktop,
  validateMacInfo,
  validateMacNativeHost,
  validateWindowsAssociations,
} from "./validate-desktop-package.mjs";

const identifier = "com.jstorrent.rstorrent";
const fileClass = `${identifier}.torrent`;

test("accepts complete macOS activation metadata", () => {
  validateMacInfo({
    CFBundleIdentifier: identifier,
    CFBundleURLTypes: [
      {
        CFBundleURLName: `${identifier} magnet`,
        CFBundleURLSchemes: ["magnet"],
      },
    ],
    CFBundleDocumentTypes: [
      {
        CFBundleTypeName: fileClass,
        CFBundleTypeExtensions: ["torrent"],
        CFBundleTypeRole: "Editor",
        LSItemContentTypes: [fileClass],
      },
    ],
    UTExportedTypeDeclarations: [
      {
        UTTypeIdentifier: fileClass,
        UTTypeConformsTo: ["public.data"],
        UTTypeTagSpecification: {
          "public.mime-type": "application/x-bittorrent",
          "public.filename-extension": ["torrent"],
        },
      },
    ],
  });
});

test("rejects incomplete macOS activation metadata", () => {
  assert.throws(
    () => validateMacInfo({ CFBundleIdentifier: identifier }),
    /does not register magnet/,
  );
});

test("requires an executable native host in the macOS application", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "rstorrent-mac-package-"));
  try {
    const executableDirectory = path.join(directory, "Contents", "MacOS");
    fs.mkdirSync(executableDirectory, { recursive: true });
    const nativeHost = path.join(executableDirectory, "rstorrent-native-host");
    fs.writeFileSync(nativeHost, "native host");
    assert.throws(() => validateMacNativeHost(directory), /must be executable/);
    fs.chmodSync(nativeHost, 0o755);
    assert.equal(validateMacNativeHost(directory), nativeHost);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("accepts a Linux handler that forwards one URL list", () => {
  validateLinuxDesktop(`[Desktop Entry]
Name=RSTorrent
Exec=rstorrent-desktop %U
Terminal=false
Type=Application
MimeType=application/x-bittorrent;x-scheme-handler/magnet;
`);
});

test("rejects a Linux handler that advertises but drops activations", () => {
  assert.throws(
    () =>
      validateLinuxDesktop(`[Desktop Entry]
Name=RSTorrent
Exec=rstorrent-desktop
Terminal=false
Type=Application
MimeType=application/x-bittorrent;x-scheme-handler/magnet;
`),
    /must forward one URL\/file list with %U/,
  );
});

test("accepts exact quoted Windows association commands", () => {
  validateWindowsAssociations({
    executable: "C:\\Users\\Test User\\AppData\\Local\\RSTorrent\\rstorrent-desktop.exe",
    torrentProgId: fileClass,
    torrentCommand:
      '"C:\\Users\\Test User\\AppData\\Local\\RSTorrent\\rstorrent-desktop.exe" "%1"',
    magnetUrlProtocol: "",
    magnetCommand:
      '"C:\\Users\\Test User\\AppData\\Local\\RSTorrent\\rstorrent-desktop.exe" "%1"',
  });
});

test("rejects an unquoted Windows executable", () => {
  assert.throws(
    () =>
      validateWindowsAssociations({
        executable: "C:\\Users\\Test User\\rstorrent-desktop.exe",
        torrentProgId: fileClass,
        torrentCommand: 'C:\\Users\\Test User\\rstorrent-desktop.exe "%1"',
        magnetUrlProtocol: "",
        magnetCommand: '"C:\\Users\\Test User\\rstorrent-desktop.exe" "%1"',
      }),
    /unexpected Windows torrent command/,
  );
});
