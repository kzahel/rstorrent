import styles from "./CrostiniStorageHelp.module.css";

const LINUX_DOWNLOADS = /^\/home\/[^/]+\/Downloads(?:\/.*)?$/u;

export function describeCrostiniStoragePath(
  path: string | null,
): string | null {
  if (path === null) return null;
  if (path === "/mnt/chromeos" || path.startsWith("/mnt/chromeos/")) {
    return "ChromeOS shared folder — convenient, but slower";
  }
  if (path === "~/Downloads" || LINUX_DOWNLOADS.test(path)) {
    return "Linux Downloads — faster (recommended)";
  }
  return null;
}

export function CrostiniStorageHelp() {
  return (
    <aside className={styles.callout} aria-label="Chromebook storage guidance">
      <strong>Linux Downloads is faster</strong>
      <p>
        Keep downloads in Linux for the best download, checking, and seeding
        performance. ChromeOS Files already shows them under <b>Linux files</b>
        {" → "}
        <b>Downloads</b>; no sharing step is required.
      </p>
      <details>
        <summary>How to use a folder from My files</summary>
        <div className={styles.instructions}>
          <p>ChromeOS calls this permission “Share with Linux”:</p>
          <ol>
            <li>Open the ChromeOS Files app.</li>
            <li>
              Under <b>My files</b>, right-click <b>Downloads</b> or another
              folder and choose <b>Share with Linux</b>.
            </li>
            <li>
              Return to RSTorrent and choose <b>Choose folder…</b> or
              {" "}
              <b>Add folder…</b>.
            </li>
            <li>
              In the folder picker, select the folder you just shared. For
              example, choose <b>Downloads</b> if you shared
              {" "}
              <b>My files</b> {" → "} <b>Downloads</b>.
            </li>
          </ol>
          <p>
            A shared ChromeOS folder is easier to use directly from My files,
            but torrent writes, verification, reading, and seeding can be much
            slower than Linux storage.
          </p>
        </div>
      </details>
    </aside>
  );
}
