// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { FileRow, FileSet, TorrentRow } from "../model";
import { PendingFileSelectionDialog } from "./PendingFileSelectionDialog";

afterEach(cleanup);

describe("pending file selection", () => {
  it("uses normal-or-skip checkboxes and sends compact ranges", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn(async () => undefined);
    renderDialog({ onConfirm });

    await user.click(screen.getByRole("checkbox", { name: /first\.mkv/i }));
    await user.click(screen.getByRole("checkbox", { name: /second\.srt/i }));
    await user.click(
      screen.getByRole("checkbox", { name: /Don’t show file selection again/i }),
    );
    await user.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(onConfirm).toHaveBeenCalledOnce());
    expect(onConfirm).toHaveBeenCalledWith(
      "current",
      [{ start: 0, endExclusive: 2, selected: false }],
      true,
    );
  });

  it("supports select-none plus sparse exceptions", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn(async () => undefined);
    renderDialog({ onConfirm });

    await user.click(screen.getByRole("button", { name: "None" }));
    await user.click(screen.getByRole("checkbox", { name: /second\.srt/i }));
    expect(screen.getByText(/1 of 2 selected/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() => expect(onConfirm).toHaveBeenCalledOnce());
    expect(onConfirm).toHaveBeenCalledWith(
      "none",
      [{ start: 1, endExclusive: 2, selected: true }],
      false,
    );
  });

  it("waits for magnet metadata and offers explicit cancellation", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn(async () => undefined);
    renderDialog({ files: metadataPendingFiles(), onCancel });

    expect(screen.getByRole("status")).toHaveTextContent(
      "No content files will download before you confirm",
    );
    expect(screen.getByRole("button", { name: "Download" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(onCancel).toHaveBeenCalledOnce());
  });

  it("loads the next bounded page near the end of the continuous list", () => {
    const onPage = vi.fn();
    renderDialog({ onPage });
    const list = screen.getByText("Scroll to load more files").parentElement!;
    Object.defineProperties(list, {
      scrollHeight: { value: 1_000 },
      scrollTop: { value: 700, configurable: true },
      clientHeight: { value: 200 },
    });

    fireEvent.scroll(list);

    expect(onPage).toHaveBeenCalledWith(2);
  });
});

function renderDialog(
  overrides: Partial<
    React.ComponentProps<typeof PendingFileSelectionDialog>
  > = {},
) {
  return render(
    <PendingFileSelectionDialog
      torrent={torrent()}
      files={availableFiles()}
      rootLabel="Downloads"
      queuedCount={0}
      dataUnits="decimal"
      onPage={() => undefined}
      onConfirm={async () => undefined}
      onCancel={async () => undefined}
      {...overrides}
    />,
  );
}

function torrent(): TorrentRow {
  return {
    id: "t1-00000000000000000000000000000000",
    name: "Example torrent",
    awaitingFileSelection: true,
    pendingFileSelectionPosition: 0,
    fileCatalogId: "a".repeat(64),
    selectableFileCount: 2,
    selectedFileCount: 2,
    selectableFileBytes: "1200",
    selectedFileBytes: "1200",
  } as TorrentRow;
}

function availableFiles(): FileSet {
  const first = file(0, "first.mkv", "1000");
  const second = file(1, "second.srt", "200");
  return {
    state: "available",
    filesystemContentBase: null,
    page: { offset: 0, limit: 1_024, total: 3, nextOffset: 2 },
    order: [first.id, second.id],
    rows: { [first.id]: first, [second.id]: second },
  };
}

function metadataPendingFiles(): FileSet {
  return {
    state: "metadata_pending",
    filesystemContentBase: null,
    page: { offset: 0, limit: 1_024, total: 0, nextOffset: null },
    order: [],
    rows: {},
  };
}

function file(index: number, name: string, lengthBytes: string): FileRow {
  return {
    id: String(index),
    torrentId: "t1-00000000000000000000000000000000",
    index,
    path: [name],
    name,
    folder: "",
    extension: name.split(".").at(-1) ?? "",
    lengthBytes,
    torrentOffsetBytes: index === 0 ? "0" : "1000",
    firstPiece: 0,
    lastPiece: 0,
    selection: "normal",
    padding: false,
    doneBytes: "0",
    verifiedBytes: "0",
    mediaAvailability: "incomplete",
    storagePath: null,
  };
}
