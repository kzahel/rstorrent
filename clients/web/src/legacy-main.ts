import "./styles.css";

import type {
  Command,
  DiagnosticCategory,
  DiagnosticEvent,
  DiagnosticProfile,
  DiagnosticSeverity,
  DiagnosticValue,
  RequestEnvelope,
  TorrentView,
} from "./api";
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
let diagnosticsSubscription: ApplicationSubscription | undefined;
let state = emptyApplicationViewState();
let selectedTorrent: string | undefined;
let nextRequest = 1;
let interopComplete = false;
let interopShutdownRequested = false;
let diagnosticProfile: DiagnosticProfile = "normal";
let diagnosticSeverity: DiagnosticSeverity = "info";
let diagnosticCategories: DiagnosticCategory[] = [];
let diagnosticScope: "global" | "torrent" = "global";
let diagnosticSearch = "";
let diagnosticAutoscroll = true;
let diagnosticResets = 0;
const diagnosticCategoryOptions: DiagnosticCategory[] = [
  "lifecycle",
  "discovery",
  "tracker",
  "peer",
  "metadata",
  "protocol",
  "scheduler",
  "piece",
  "storage",
  "integrity",
  "platform",
  "performance",
];
const interop =
  import.meta.env.DEV && import.meta.env.VITE_RSTORRENT_INTEROP_MAGNET
    ? {
        magnet: import.meta.env.VITE_RSTORRENT_INTEROP_MAGNET,
        gatewayUrl: import.meta.env.VITE_RSTORRENT_INTEROP_GATEWAY_URL,
        gatewayToken: import.meta.env.VITE_RSTORRENT_INTEROP_GATEWAY_TOKEN,
        externalControl:
          import.meta.env.VITE_RSTORRENT_INTEROP_EXTERNAL_CONTROL === "1",
        expectTrackerRetry:
          import.meta.env.VITE_RSTORRENT_INTEROP_EXPECT_TRACKER_RETRY === "1",
        requested: 0,
        received: 0,
        stored: 0,
        control: "waiting" as
          | "waiting"
          | "pause_requested"
          | "paused"
          | "resume_requested"
          | "resumed"
          | "retry_waiting",
      }
    : undefined;

if ("__TAURI_INTERNALS__" in window) {
  void import("./tauri-client").then(({ TauriApplicationClient }) => {
    void startClient(new TauriApplicationClient());
  });
} else if (
  interop?.gatewayUrl !== undefined &&
  interop.gatewayToken !== undefined
) {
  void connect(interop.gatewayUrl, interop.gatewayToken);
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
        min_interval_millis: interop === undefined ? 250 : 0,
        max_queue_bytes: 256 * 1024,
      },
    });
    renderApplication();
    void consume(listSubscription);
    await subscribeDiagnostics();
    if (interop !== undefined) {
      void dispatch({
        type: "add_magnet",
        magnet: interop.magnet,
        storage_root: "downloads",
        skip_files: [],
      });
    }
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
          diagnosticResets += 1;
          renderApplication();
          await subscription.resync();
          continue;
        }
        throw error;
      }
      observeInterop();
      if (interop !== undefined && selectedTorrent === undefined) {
        const first = Object.keys(state.torrents)[0];
        if (first !== undefined) await selectTorrent(first);
      }
      void exerciseInteropControl();
      void finishInteropIfReady();
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
  const selectedView =
    selectedTorrent === undefined ? undefined : state.torrents[selectedTorrent];
  root.innerHTML = `
    <header class="app-header">
      <div><span class="brand-dot"></span><strong>RSTorrent</strong></div>
      <span class="connection-state">Connected</span>
    </header>
    ${
      interopComplete && interop !== undefined
        ? `<output id="interop-result" data-complete="true" ` +
          `data-requested="${interop.requested}" ` +
          `data-received="${interop.received}" ` +
          `data-stored="${interop.stored}" ` +
          `data-control="${interop.control}" ` +
          `data-progress="${selectedView?.progress.disposition ?? ""}" ` +
          `data-reason="${selectedView?.progress.reason ?? ""}">controlled scenario complete</output>`
        : ""
    }
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
        ${selectedView === undefined ? "" : renderProgressDetail(selectedView)}
        ${selected === undefined ? "" : renderPieceActivity(selected)}
        ${renderDiagnostics()}
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
        if (interop?.externalControl === true) {
          interop.control =
            type === "pause" ? "pause_requested" : "resume_requested";
        }
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
  bindDiagnosticControls();
  if (selected !== undefined) {
    drawPieceMap(selected);
  }
  if (diagnosticAutoscroll) {
    root.querySelector<HTMLElement>(".diagnostic-events")?.scrollTo({
      top: Number.MAX_SAFE_INTEGER,
    });
  }
}

function observeInterop(): void {
  if (interop === undefined) return;
  for (const activity of Object.values(state.pieces)) {
    for (const active of activity.active) {
      interop.requested = Math.max(interop.requested, rangeBytes(active.requested));
      interop.received = Math.max(interop.received, rangeBytes(active.received));
      interop.stored = Math.max(interop.stored, rangeBytes(active.stored));
    }
  }
}

async function exerciseInteropControl(): Promise<void> {
  if (interop === undefined) return;
  const torrent = Object.values(state.torrents)[0];
  if (torrent === undefined) return;
  if (interop.externalControl) {
    if (
      interop.control === "pause_requested" &&
      torrent.state === "paused"
    ) {
      interop.control = "paused";
    } else if (
      interop.control === "resume_requested" &&
      torrent.state === "downloading"
    ) {
      interop.control = "resumed";
    }
    return;
  }
  if (interop.control === "waiting" && torrent.state === "downloading") {
    interop.control = "pause_requested";
    await dispatch({ type: "pause", torrent_id: torrent.torrent_id });
  } else if (
    interop.control === "pause_requested" &&
    torrent.state === "paused"
  ) {
    interop.control = "resume_requested";
    await dispatch({ type: "resume", torrent_id: torrent.torrent_id });
  } else if (
    interop.control === "resume_requested" &&
    torrent.state === "downloading"
  ) {
    interop.control = "resumed";
  }
}

async function finishInteropIfReady(): Promise<void> {
  if (
    interop !== undefined &&
    interop.expectTrackerRetry &&
    !interopComplete &&
    Object.values(state.torrents).some(
      (torrent) =>
        torrent.progress.disposition === "waiting" &&
        torrent.progress.reason === "waiting_for_discovery",
    ) &&
    state.diagnostics.some((event) => event.code === "tracker_retry_scheduled")
  ) {
    interop.control = "retry_waiting";
    interopComplete = true;
    renderApplication();
    return;
  }
  if (
    interop === undefined ||
    interop.expectTrackerRetry ||
    interopShutdownRequested ||
    interop.control !== "resumed" ||
    !Object.values(state.torrents).some((torrent) => torrent.state === "complete") ||
    interop.requested === 0 ||
    interop.received === 0 ||
    interop.stored === 0
  ) {
    return;
  }
  interopComplete = true;
  renderApplication();
  if ("__TAURI_INTERNALS__" in window) {
    interopShutdownRequested = true;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("application_shutdown");
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
        <p class="progress-reason disposition-${torrent.progress.disposition}">${torrent.progress.disposition} · ${humanize(torrent.progress.phase)} · ${humanize(torrent.progress.reason)}</p>
        ${torrent.error == null ? "" : `<p class="torrent-error">${escapeHtml(torrent.error)}</p>`}
      </div>
      <div class="torrent-progress">
        <strong>${percent.toFixed(2)}%</strong>
        <div class="progress-track"><i style="width:${percent}%"></i></div>
      </div>
      <button class="quiet" data-command="${command}" data-torrent="${torrent.torrent_id}">${command}</button>
    </article>
  `;
}

function renderProgressDetail(torrent: TorrentView): string {
  const actions =
    torrent.progress.actions.length === 0
      ? ""
      : `<p>Suggested: ${torrent.progress.actions.map(humanize).join(", ")}</p>`;
  return `
    <section class="progress-panel panel disposition-${torrent.progress.disposition}" data-progress-disposition="${torrent.progress.disposition}">
      <div><p class="eyebrow">Progress assessment</p><h2>${humanize(torrent.progress.disposition)} · ${humanize(torrent.progress.phase)}</h2></div>
      <p>${progressExplanation(torrent)}</p>
      ${actions}
    </section>
  `;
}

function renderDiagnostics(): string {
  const events = visibleDiagnostics();
  return `
    <section class="diagnostics-panel panel" aria-label="Diagnostics">
      <div class="section-heading compact">
        <div><p class="eyebrow">Bounded timeline</p><h2>Diagnostics</h2></div>
        <span>${events.length} shown · ${state.diagnosticDropped} dropped · ${diagnosticResets} resyncs</span>
      </div>
      <div class="diagnostic-toolbar">
        <div class="profile-buttons" role="group" aria-label="Diagnostic profile">
          ${(["normal", "detailed", "trace"] as const)
            .map(
              (profile) =>
                `<button class="filter-button ${profile === diagnosticProfile ? "active" : ""}" data-profile="${profile}">${humanize(profile)}</button>`,
            )
            .join("")}
        </div>
        <label>Scope
          <select id="diagnostic-scope">
            <option value="global" ${diagnosticScope === "global" ? "selected" : ""}>Global</option>
            <option value="torrent" ${diagnosticScope === "torrent" ? "selected" : ""} ${selectedTorrent === undefined ? "disabled" : ""}>Selected torrent</option>
          </select>
        </label>
        <label>Minimum severity
          <select id="diagnostic-severity">
            ${(["trace", "debug", "info", "warning", "error"] as const)
              .map(
                (severity) =>
                  `<option value="${severity}" ${severity === diagnosticSeverity ? "selected" : ""}>${humanize(severity)}</option>`,
              )
              .join("")}
          </select>
        </label>
        <label>Search
          <input id="diagnostic-search" value="${escapeAttribute(diagnosticSearch)}" placeholder="code, category, summary" />
        </label>
        <button class="quiet" id="diagnostic-autoscroll">${diagnosticAutoscroll ? "Pause autoscroll" : "Resume autoscroll"}</button>
        <button class="quiet" id="diagnostic-copy">Copy shown</button>
      </div>
      <div class="category-filters" aria-label="Diagnostic categories">
        ${diagnosticCategoryOptions
          .map(
            (category) =>
              `<button class="filter-button ${diagnosticCategories.includes(category) ? "active" : ""}" data-category="${category}">${humanize(category)}</button>`,
          )
          .join("")}
      </div>
      ${
        diagnosticProfile === "trace"
          ? '<p class="trace-warning">Trace is high volume and lasts only for this session.</p>'
          : ""
      }
      <div class="diagnostic-events" role="log" aria-live="polite">
        ${
          events.length === 0
            ? '<p class="diagnostic-empty">No diagnostics match the current filters.</p>'
            : events.map(renderDiagnosticEvent).join("")
        }
      </div>
    </section>
  `;
}

function renderDiagnosticEvent(event: DiagnosticEvent): string {
  const fields = event.fields
    .map(
      (field) =>
        `<span><b>${escapeHtml(field.key)}</b>=${escapeHtml(diagnosticValueText(field.value))}</span>`,
    )
    .join("");
  return `
    <article class="diagnostic-event severity-${event.severity}" data-event-code="${escapeAttribute(event.code)}">
      <time>${escapeHtml(new Date(Number(event.timestamp_millis)).toLocaleTimeString())}</time>
      <strong>${escapeHtml(event.severity)}</strong>
      <code>${escapeHtml(event.category)} / ${escapeHtml(event.code)}</code>
      <p>${escapeHtml(event.message)}</p>
      ${fields === "" ? "" : `<div>${fields}</div>`}
    </article>
  `;
}

function visibleDiagnostics(): DiagnosticEvent[] {
  const needle = diagnosticSearch.trim().toLocaleLowerCase();
  return state.diagnostics.filter((event) => {
    if (
      diagnosticScope === "torrent" &&
      event.torrent_id !== undefined &&
      event.torrent_id !== null &&
      event.torrent_id !== selectedTorrent
    ) {
      return false;
    }
    if (needle === "") return true;
    return [
      event.code,
      event.category,
      event.severity,
      event.message,
      ...event.fields.flatMap((field) => [
        field.key,
        diagnosticValueText(field.value),
      ]),
    ].some((value) => value.toLocaleLowerCase().includes(needle));
  });
}

function bindDiagnosticControls(): void {
  for (const button of root.querySelectorAll<HTMLButtonElement>(
    "[data-profile]",
  )) {
    button.addEventListener("click", () => {
      diagnosticProfile = button.dataset.profile as DiagnosticProfile;
      void subscribeDiagnostics();
    });
  }
  for (const button of root.querySelectorAll<HTMLButtonElement>(
    "[data-category]",
  )) {
    button.addEventListener("click", () => {
      const category = button.dataset.category as DiagnosticCategory;
      diagnosticCategories = diagnosticCategories.includes(category)
        ? diagnosticCategories.filter((value) => value !== category)
        : [...diagnosticCategories, category];
      void subscribeDiagnostics();
    });
  }
  root
    .querySelector<HTMLSelectElement>("#diagnostic-scope")
    ?.addEventListener("change", (event) => {
      diagnosticScope = (event.currentTarget as HTMLSelectElement).value as
        | "global"
        | "torrent";
      void subscribeDiagnostics();
    });
  root
    .querySelector<HTMLSelectElement>("#diagnostic-severity")
    ?.addEventListener("change", (event) => {
      diagnosticSeverity = (event.currentTarget as HTMLSelectElement)
        .value as DiagnosticSeverity;
      void subscribeDiagnostics();
    });
  root
    .querySelector<HTMLInputElement>("#diagnostic-search")
    ?.addEventListener("change", (event) => {
      diagnosticSearch = (event.currentTarget as HTMLInputElement).value;
      renderApplication();
    });
  root
    .querySelector<HTMLButtonElement>("#diagnostic-autoscroll")
    ?.addEventListener("click", () => {
      diagnosticAutoscroll = !diagnosticAutoscroll;
      renderApplication();
    });
  root
    .querySelector<HTMLButtonElement>("#diagnostic-copy")
    ?.addEventListener("click", () => void copyDiagnostics());
}

async function subscribeDiagnostics(): Promise<void> {
  await diagnosticsSubscription?.close();
  if (client === undefined) return;
  const selector =
    diagnosticScope === "torrent" && selectedTorrent !== undefined
      ? { type: "torrent" as const, torrent_id: selectedTorrent }
      : { type: "torrent_list" as const };
  diagnosticsSubscription = await client.subscribe({
    selector,
    projection: "diagnostics",
    delivery: {
      min_interval_millis: interop === undefined ? 100 : 0,
      max_queue_bytes: 256 * 1024,
    },
    diagnostics: {
      profile: diagnosticProfile,
      minimum_severity: diagnosticSeverity,
      categories: diagnosticCategories,
    },
  });
  void consume(diagnosticsSubscription);
  renderApplication();
}

async function copyDiagnostics(): Promise<void> {
  const text = visibleDiagnostics()
    .map(
      (event) =>
        `${event.timestamp_millis} ${event.severity} ${event.category} ${event.code} ${event.message}`,
    )
    .join("\n")
    .slice(0, 64 * 1024);
  try {
    await navigator.clipboard.writeText(text);
    showStatus(`Copied ${text.length.toLocaleString()} diagnostic characters.`, false);
  } catch (error) {
    showStatus(`Copy failed: ${errorMessage(error)}`, true);
  }
}

function diagnosticValueText(value: DiagnosticValue): string {
  return value.type === "boolean" ? String(value.value) : value.value;
}

function progressExplanation(torrent: TorrentView): string {
  switch (torrent.progress.reason) {
    case "network_disabled":
      return "Outbound networking is disabled. Torrent intent is preserved and can continue when networking is enabled.";
    case "no_enabled_discovery_source":
      return "No enabled discovery source can currently provide an eligible peer. The torrent remains ready for a future discovery capability.";
    case "waiting_for_storage":
      return "Verified metadata is waiting for a download folder.";
    case "waiting_for_discovery":
      return "Another automatic discovery mechanism or retry is scheduled.";
    default:
      return humanize(torrent.progress.reason);
  }
}

function humanize(value: string): string {
  return value.replaceAll("_", " ");
}

function renderPieceActivity(activity: PieceActivityState): string {
  const active = activity.active;
  const blockSummary =
    active.length === 0
      ? "No active piece"
      : `${active.length.toLocaleString()} active ${active.length === 1 ? "piece" : "pieces"}`;
  return `
    <section class="piece-panel panel">
      <div class="section-heading compact">
        <div><p class="eyebrow">Live detail</p><h2>Piece activity</h2></div>
        <span>${blockSummary}</span>
      </div>
      <canvas id="piece-map" width="960" height="144" aria-label="Verified piece map"></canvas>
      ${active.length === 0 ? "" : renderBlockTracks(active)}
    </section>
  `;
}

function renderBlockTracks(
  active: PieceActivityState["active"],
): string {
  return `
    <div class="block-legend">
      <span><i class="requested"></i>Requested ${active.reduce((total, piece) => total + rangeBytes(piece.requested), 0).toLocaleString()}</span>
      <span><i class="received"></i>Received ${active.reduce((total, piece) => total + rangeBytes(piece.received), 0).toLocaleString()}</span>
      <span><i class="stored"></i>Stored ${active.reduce((total, piece) => total + rangeBytes(piece.stored), 0).toLocaleString()}</span>
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
      min_interval_millis: 0,
      max_queue_bytes: 256 * 1024,
    },
  });
  if (pieceSubscription !== undefined) void consume(pieceSubscription);
  if (diagnosticScope === "torrent") await subscribeDiagnostics();
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
    const active = activity.active.some(
      (piece) => piece.piece_index >= start && piece.piece_index < end,
    );
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
