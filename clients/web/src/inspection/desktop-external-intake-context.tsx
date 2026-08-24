import {
  createContext,
  useContext,
  useEffect,
  useSyncExternalStore,
  type ReactNode,
} from "react";

import type {
  DesktopExternalIntake,
  DesktopExternalIntakeSnapshot,
} from "../desktop-external-intake";

const EMPTY_DESKTOP_EXTERNAL_INTAKE_SNAPSHOT: DesktopExternalIntakeSnapshot = {
  generation: "0",
  pending: [],
  rejectedCount: 0,
  overflowCount: 0,
};

const DesktopExternalIntakeContext =
  createContext<DesktopExternalIntake | null>(null);

export function DesktopExternalIntakeProvider({
  intake,
  children,
}: {
  readonly intake?: DesktopExternalIntake | undefined;
  readonly children: ReactNode;
}) {
  useEffect(() => () => intake?.close(), [intake]);
  return (
    <DesktopExternalIntakeContext.Provider value={intake ?? null}>
      {children}
    </DesktopExternalIntakeContext.Provider>
  );
}

export function useDesktopExternalIntake(): {
  readonly intake: DesktopExternalIntake | null;
  readonly snapshot: DesktopExternalIntakeSnapshot;
} {
  const intake = useContext(DesktopExternalIntakeContext);
  const snapshot = useSyncExternalStore(
    intake?.subscribe ?? emptySubscribe,
    intake?.getSnapshot ?? emptySnapshot,
    emptySnapshot,
  );
  return { intake, snapshot };
}

function emptySubscribe(): () => void {
  return () => undefined;
}

function emptySnapshot(): DesktopExternalIntakeSnapshot {
  return EMPTY_DESKTOP_EXTERNAL_INTAKE_SNAPSHOT;
}
