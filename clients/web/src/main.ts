import "./styles.css";

import type {
  Command,
  RequestEnvelope,
  TorrentView,
} from "./generated/contract";
import type {
  ApplicationClient,
  ApplicationSubscription,
} from "./application-client";
import {
  ResetRequiredError,
  emptyApplicationViewState,
  reduceViewUpdate,
  type ApplicationViewState,
  type PieceActivityState,
} from "./reducer";
import { WebSocketApplicationClient } from "./websocket-client";

const rootElement = document.querySelector<HTMLElement>("#app");
if (rootElement === null) throw new Error("missing application root");
const root: HTMLElement = rootElement;

let client: ApplicationClient | undefined;
let listSubscription: ApplicationSubscription | undefined;
let pieceSubscription: ApplicationSubscription | undefined;
let state = emptyApplicationViewState();
let selectedTorrent: string | undefined;
let nextRequest = 1;

if ("__TAURI_INTERNALS__" in window) {
  void import("./tauri-client").then(({ TauriApplicationClient }) => {
    void startClient(new TauriApplicationClient());
  });
} else {
  renderConnect();
}

function renderConnect(message = ""): void {
  const defaultUrl = `ws://${window.location.hostname || "127.0.0.1"}:3030/control`;
  root.innerHTML = `
    <section class="connect-shell">
      <div class="brand-mark" aria-hidden="true">RS</div>
      <div>
        <p class="eyebrow">Remote proof</p>
        <h1>RSTorrent</h1>
        <p class="lede">One bounded control surface for the first-party engine.</p>
      </div>
      <form id="connect-form" class="panel connect-form">
        <label>Gateway URL<input name="url" value="${escapeAttribute(defaultUrl)}" required /></label>
        <label>Session token<input name="token" type="password" autocomplete="off" required /></label>
        <button type="submit">Connect</button>
        <p class="form-message">${escapeHtml(message)}</p>
      </form>
    </section>
  `;
  root
    .querySelector<HTMLFormElement>("#connect-form")
    ?.addEventListener("submit", (event) => {
      event.preventDefault();
      const form = new FormData(event.currentTarget as HTMLFormElement);
      const url = String(form.get("url") ?? "");
      const token = String(form.get("token") ?? "");
      void connect(url, token);
    });
}

async function connect(url: string, token: string): Promise<void> {
  renderConnect("Connecting…");
  try {
    await startClient(await WebSocketApplicationClient.connect(url, token));
  } catch (error) {
    renderConnect(errorMessage(error));
  }
}

async function startClient(applicationClient: ApplicationClient): Promise<void> {
  try {
    client = applicationClient;
    listSubscription = await applicationClient.subscribe({
      selector: { type: "torrent_list" },
      projection: "summary",
      delivery: {
        min_interval_millis: 250,
        max_queue_bytes: 256 * 1024,
      },
    });
    renderApplication();
    void consume(listSubscription);
  } catch (error) {
    await applicationClient.close();
    throw error;
  }
}

async function consume(subscription: ApplicationSubscription): Promise<void> {
  try {
    for await (const update of subscription) {
      try {
        state = reduceViewUpdate(state, update);
      } catch (error) {
        if (error instanceof ResetRequiredError) {
          await subscription.resync();
          continue;
        }
        throw error;
      }
      renderApplication();
    }
  } catch (error) {
    showStatus(errorMessage(error), true);
  }
}

function renderApplication(): void {
  const torrents = Object.values(state.torrents);
  const selected =
    selectedTorrent === undefined ? undefined : state.pieces[selectedTorrent];
  root.innerHTML = `
    <header class="app-header">
      <div><span class="brand-dot"></span><strong>RSTorrent</strong></div>
      <span class="connection-state">Connected</span>
    </header>
    <section class="workspace">
      <aside>
        <form id="magnet-form" class="panel magnet-form">
          <p class="eyebrow">New transfer</p>
          <label>Magnet link<textarea name="magnet" rows="4" required placeholder="magnet:?xt=urn:btih:…"></textarea></label>
          <button type="submit">Add magnet</button>
        </form>
        <div id="status" class="status" role="status"></div>
      </aside>
      <section class="transfer-column">
        <div class="section-heading">
          <div><p class="eyebrow">Profile</p><h1>Transfers</h1></div>
          <span>${torrents.length} torrent${torrents.length === 1 ? "" : "s"}</span>
        </div>
        <div class="torrent-list">
          ${torrents.length === 0 ? '<div class="empty panel">No torrents yet. Add a controlled magnet to begin.</div>' : torrents.map(renderTorrent).join("")}
        </div>
        ${selected === undefined ? "" : renderPieceActivity(selected)}
      </section>
    </section>
  `;
  root
    .querySelector<HTMLFormElement>("#magnet-form")
    ?.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(event.currentTarget as HTMLFormElement);
      void dispatch({
        type: "add_magnet",
        magnet: String(data.get("magnet") ?? ""),
        storage_root: "downloads",
        skip_files: [],
      });
    });
  for (const button of root.querySelectorAll<HTMLButtonElement>(
    "[data-command]",
  )) {
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      const torrentId = button.dataset.torrent;
      if (torrentId === undefined) return;
      const type = button.dataset.command;
      if (type === "pause" || type === "resume") {
        void dispatch({ type, torrent_id: torrentId });
      }
    });
  }
  for (const card of root.querySelectorAll<HTMLElement>("[data-select]")) {
    card.addEventListener("click", () => {
      const torrentId = card.dataset.select;
      if (torrentId !== undefined) void selectTorrent(torrentId);
    });
  }
  if (selected !== undefined) {
    drawPieceMap(selected);
  }
}

function renderTorrent(torrent: TorrentView): string {
  const percent =
    torrent.piece_count === 0
      ? 0
      : Math.round(
          (torrent.verified_piece_count / torrent.piece_count) * 10_000,
        ) / 100;
  const command = torrent.state === "paused" ? "resume" : "pause";
  return `
    <article class="torrent-card panel ${selectedTorrent === torrent.torrent_id ? "selected" : ""}" data-select="${torrent.torrent_id}" tabindex="0">
      <div class="torrent-main">
        <span class="state-pill state-${torrent.state}">${torrent.state.replaceAll("_", " ")}</span>
        <h2>${torrent.torrent_id.slice(0, 12)}<span>${torrent.torrent_id.slice(12)}</span></h2>
        <p>${torrent.verified_piece_count.toLocaleString()} / ${torrent.piece_count.toLocaleString()} pieces</p>
      </div>
      <div class="torrent-progress">
        <strong>${percent.toFixed(2)}%</strong>
        <div class="progress-track"><i style="width:${percent}%"></i></div>
      </div>
      <button class="quiet" data-command="${command}" data-torrent="${torrent.torrent_id}">${command}</button>
    </article>
  `;
}

function renderPieceActivity(activity: PieceActivityState): string {
  const active = activity.active;
  const blockSummary =
    active === null
      ? "No active piece"
      : `Piece ${active.piece_index.toLocaleString()} · ${active.piece_length.toLocaleString()} bytes`;
  return `
    <section class="piece-panel panel">
      <div class="section-heading compact">
        <div><p class="eyebrow">Live detail</p><h2>Piece activity</h2></div>
        <span>${blockSummary}</span>
      </div>
      <canvas id="piece-map" width="960" height="144" aria-label="Verified piece map"></canvas>
      ${active === null ? "" : renderBlockTracks(active)}
    </section>
  `;
}

function renderBlockTracks(
  active: NonNullable<PieceActivityState["active"]>,
): string {
  return `
    <div class="block-legend">
      <span><i class="requested"></i>Requested ${rangeBytes(active.requested).toLocaleString()}</span>
      <span><i class="received"></i>Received ${rangeBytes(active.received).toLocaleString()}</span>
      <span><i class="stored"></i>Stored ${rangeBytes(active.stored).toLocaleString()}</span>
    </div>
  `;
}

async function selectTorrent(torrentId: string): Promise<void> {
  if (selectedTorrent === torrentId) return;
  await pieceSubscription?.close();
  selectedTorrent = torrentId;
  pieceSubscription = await client?.subscribe({
    selector: { type: "torrent", torrent_id: torrentId },
    projection: "piece_activity",
    delivery: {
      min_interval_millis: 50,
      max_queue_bytes: 256 * 1024,
    },
  });
  if (pieceSubscription !== undefined) void consume(pieceSubscription);
  renderApplication();
}

async function dispatch(command: Command): Promise<void> {
  if (client === undefined) return;
  const request: RequestEnvelope = {
    version: 1,
    request_id: `web-${nextRequest}`,
    command,
  };
  nextRequest += 1;
  try {
    const response = await client.dispatch(request);
    if (response.status === "error") throw new Error(response.error.message);
    showStatus(`Revision ${response.revision} accepted.`, false);
  } catch (error) {
    showStatus(errorMessage(error), true);
  }
}

function drawPieceMap(activity: PieceActivityState): void {
  const canvas = root.querySelector<HTMLCanvasElement>("#piece-map");
  if (canvas === null) return;
  const context = canvas.getContext("2d");
  if (context === null) return;
  const columns = 160;
  const rows = 24;
  const buckets = columns * rows;
  const bucketPieces = Math.max(1, Math.ceil(activity.pieceCount / buckets));
  context.clearRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = "#1d2630";
  context.fillRect(0, 0, canvas.width, canvas.height);
  let rangeIndex = 0;
  for (let bucket = 0; bucket < buckets; bucket += 1) {
    const start = bucket * bucketPieces;
    const end = Math.min(activity.pieceCount, start + bucketPieces);
    while (true) {
      const currentRange = activity.verified[rangeIndex];
      if (currentRange === undefined || currentRange.end_exclusive > start) {
        break;
      }
      rangeIndex += 1;
    }
    const range = activity.verified[rangeIndex];
    const verified =
      range !== undefined && range.start < end && range.end_exclusive > start;
    const active =
      activity.active !== null &&
      activity.active.piece_index >= start &&
      activity.active.piece_index < end;
    context.fillStyle = active ? "#e9aa4f" : verified ? "#55d6a7" : "#293541";
    const x = (bucket % columns) * 6;
    const y = Math.floor(bucket / columns) * 6;
    context.fillRect(x + 1, y + 1, 4, 4);
  }
}

function rangeBytes(
  ranges: ReadonlyArray<{ start: number; end_exclusive: number }>,
): number {
  return ranges.reduce(
    (sum, range) => sum + range.end_exclusive - range.start,
    0,
  );
}

function showStatus(message: string, error: boolean): void {
  const status = root.querySelector<HTMLElement>("#status");
  if (status === null) return;
  status.textContent = message;
  status.classList.toggle("error", error);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function escapeHtml(value: string): string {
  const element = document.createElement("div");
  element.textContent = value;
  return element.innerHTML;
}

function escapeAttribute(value: string): string {
  return escapeHtml(value).replaceAll('"', "&quot;");
}
