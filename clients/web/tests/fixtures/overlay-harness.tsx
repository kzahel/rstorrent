import { useState } from "react";
import { createRoot } from "react-dom/client";

import "../../src/inspection/global.css";
import {
  ActionMenuItem,
  ActionMenuPopover,
  ActionMenuTrigger,
  ActionSubmenu,
  OverlayButton,
} from "../../src/inspection/components/overlays/AnchoredOverlay";
import type {
  ColorTheme,
  InterfaceSize,
} from "../../src/inspection/appearance";
import "./overlay-harness.css";

const parameters = new URLSearchParams(location.search);
const corner = parameters.get("corner") ?? "top-left";
const mode = parameters.get("mode") === "context" ? "contextMenu" : "press";
const interfaceSize = (parameters.get("size") ?? "standard") as InterfaceSize;
const colorTheme = (parameters.get("theme") ?? "light") as ColorTheme;

document.documentElement.dataset.interfaceSize = interfaceSize;
document.documentElement.dataset.colorTheme = colorTheme;

function Harness() {
  const [mounted, setMounted] = useState(true);
  const [disabled, setDisabled] = useState(false);
  const [outsideCount, setOutsideCount] = useState(0);

  return (
    <main className="harness">
      <div className="target" data-corner={corner}>
        {mounted ? (
          <ActionMenuTrigger trigger={mode} isDisabled={disabled}>
            <OverlayButton isDisabled={disabled}>Harness actions</OverlayButton>
            <ActionMenuPopover>
              <ActionMenuItem>Copy address</ActionMenuItem>
              <ActionSubmenu trigger="Add test torrent">
                <ActionMenuItem>Big Buck Bunny</ActionMenuItem>
                <ActionMenuItem>Cosmos Laundromat</ActionMenuItem>
                <ActionMenuItem>Sintel</ActionMenuItem>
                <ActionMenuItem>Tears of Steel</ActionMenuItem>
                <ActionMenuItem>WIRED CD</ActionMenuItem>
              </ActionSubmenu>
              <ActionMenuItem>Normal priority</ActionMenuItem>
              <ActionMenuItem>Skip this file</ActionMenuItem>
              <ActionMenuItem>Verify downloaded pieces</ActionMenuItem>
              <ActionMenuItem>Reveal containing folder</ActionMenuItem>
              <ActionMenuItem>Copy magnet link</ActionMenuItem>
              <ActionMenuItem>Inspect transfer details</ActionMenuItem>
              <ActionMenuItem>Move to another folder</ActionMenuItem>
              <ActionMenuItem>Reset transfer statistics</ActionMenuItem>
              <ActionMenuItem>Archive completed torrent</ActionMenuItem>
            </ActionMenuPopover>
          </ActionMenuTrigger>
        ) : null}
      </div>
      <div className="controls">
        <button
          type="button"
          data-testid="outside-action"
          onClick={() => setOutsideCount((count) => count + 1)}
        >
          Outside action
        </button>
        <button
          type="button"
          data-testid="disable-trigger"
          onClick={() => setDisabled(true)}
        >
          Disable trigger
        </button>
        <button
          type="button"
          data-testid="unmount-owner"
          onClick={() => setMounted(false)}
        >
          Unmount owner
        </button>
        <output aria-label="Outside action count">{outsideCount}</output>
      </div>
    </main>
  );
}

createRoot(document.querySelector("#overlay-harness")!).render(<Harness />);
