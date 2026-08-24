import { describe, expect, it, vi } from "vitest";
import type { InvokeArgs } from "@tauri-apps/api/core";

import {
  TauriDesktopExternalIntake,
  decodeExternalIntakeSnapshot,
  type DesktopExternalIntakeBridge,
} from "./desktop-external-intake";

const firstId = "00010203-0405-4607-8809-0a0b0c0d0e0f";
const secondId = "11110203-0405-4607-8809-0a0b0c0d0e0f";

class FakeBridge implements DesktopExternalIntakeBridge {
  readonly calls: Array<{
    readonly command: string;
    readonly arguments_: InvokeArgs | undefined;
  }> = [];
  readonly order: string[] = [];
  handler: (command: string, arguments_?: InvokeArgs) => unknown | Promise<unknown> =
    () => snapshot("0", []);
  eventHandler: (() => void) | null = null;
  unlisten = vi.fn();

  async invoke<T>(command: string, arguments_?: InvokeArgs): Promise<T> {
    this.order.push(`invoke:${command}`);
    this.calls.push({ command, arguments_ });
    return (await this.handler(command, arguments_)) as T;
  }

  async listen(event: string, handler: () => void): Promise<() => void> {
    this.order.push(`listen:${event}`);
    this.eventHandler = handler;
    return this.unlisten;
  }
}

describe("desktop external torrent intake", () => {
  it("installs the signal listener before pulling cold pending activations", async () => {
    const bridge = new FakeBridge();
    bridge.handler = () =>
      snapshot("1", [
        { id: firstId, kind: "magnet" },
        { id: secondId, kind: "torrent_file" },
      ]);

    const intake = await TauriDesktopExternalIntake.open(bridge);

    expect(bridge.order).toEqual([
      "listen:rstorrent://external-torrent-intake",
      "invoke:desktop_external_intake_pull",
    ]);
    expect(intake.getSnapshot().pending).toEqual([
      { id: firstId, kind: "magnet" },
      { id: secondId, kind: "torrent_file" },
    ]);
    intake.close();
    expect(bridge.unlisten).toHaveBeenCalledOnce();
  });

  it("re-pulls when a signal races an in-flight cold pull", async () => {
    const bridge = new FakeBridge();
    const firstPull = promiseWithResolvers<unknown>();
    let pulls = 0;
    bridge.handler = () => {
      pulls += 1;
      return pulls === 1
        ? firstPull.promise
        : snapshot("2", [{ id: secondId, kind: "torrent_file" }]);
    };
    const opening = TauriDesktopExternalIntake.open(bridge);
    await vi.waitFor(() => expect(bridge.eventHandler).not.toBeNull());
    bridge.eventHandler?.();
    firstPull.resolve(snapshot("1", [{ id: firstId, kind: "magnet" }]));

    const intake = await opening;

    expect(pulls).toBe(2);
    expect(intake.getSnapshot()).toMatchObject({
      generation: "2",
      pending: [{ id: secondId, kind: "torrent_file" }],
    });
    intake.close();
  });

  it("cancels only an opaque activation ID and drains notices locally", async () => {
    const bridge = new FakeBridge();
    let current = snapshot("3", [{ id: firstId, kind: "magnet" }], 2, 1);
    bridge.handler = (command) => {
      if (command === "desktop_external_intake_cancel") {
        current = snapshot("4", []);
        return undefined;
      }
      return current;
    };
    const intake = await TauriDesktopExternalIntake.open(bridge);
    const updates = vi.fn();
    intake.subscribe(updates);

    intake.consumeNotices();
    expect(intake.getSnapshot()).toMatchObject({
      rejectedCount: 0,
      overflowCount: 0,
    });
    expect(updates).toHaveBeenCalledOnce();
    await intake.cancel(firstId);
    expect(intake.getSnapshot().pending).toEqual([]);
    expect(bridge.calls).toContainEqual({
      command: "desktop_external_intake_cancel",
      arguments_: { activationId: firstId },
    });
    expect(JSON.stringify(bridge.calls)).not.toContain("magnet:?");
    intake.close();
  });

  it("rejects oversized, duplicate, malformed, and content-bearing descriptors", () => {
    expect(() =>
      decodeExternalIntakeSnapshot(
        snapshot(
          "1",
          Array.from({ length: 9 }, (_, index) => ({
            id: `${String(index).padStart(8, "0")}-0405-4607-8809-0a0b0c0d0e0f`,
            kind: "magnet",
          })),
        ),
      ),
    ).toThrow("pending queue");
    expect(() =>
      decodeExternalIntakeSnapshot(
        snapshot("1", [
          { id: firstId, kind: "magnet" },
          { id: firstId, kind: "magnet" },
        ]),
      ),
    ).toThrow("duplicated");
    expect(() =>
      decodeExternalIntakeSnapshot({
        ...snapshot("1", []),
        path: "/private/source.torrent",
      }),
    ).toThrow("fields");
    expect(() =>
      decodeExternalIntakeSnapshot(
        snapshot("1", [
          {
            id: firstId,
            kind: "magnet",
            magnet: "magnet:?xt=private",
          },
        ]),
      ),
    ).toThrow("fields");
    expect(() =>
      decodeExternalIntakeSnapshot(snapshot("01", [])),
    ).toThrow("generation");
  });
});

function snapshot(
  generation: string,
  pending: readonly Record<string, unknown>[],
  rejectedCount = 0,
  overflowCount = 0,
): Record<string, unknown> {
  return { generation, pending, rejectedCount, overflowCount };
}

function promiseWithResolvers<T>(): {
  readonly promise: Promise<T>;
  resolve(value: T): void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolved) => {
    resolve = resolved;
  });
  return { promise, resolve };
}
