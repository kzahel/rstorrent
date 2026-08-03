import {
  createContext,
  useContext,
  useEffect,
  type ReactNode,
} from "react";
import { useStore } from "zustand";

import { InspectionController } from "./controller";
import type { CommandResult, InspectionCommand } from "./model";
import type { InspectionStore } from "./state";

const InspectionContext = createContext<InspectionController | null>(null);

export interface InspectionProviderProps {
  readonly controller: InspectionController;
  readonly children: ReactNode;
}

export function InspectionProvider({
  controller,
  children,
}: InspectionProviderProps) {
  useEffect(() => () => void controller.close(), [controller]);
  return (
    <InspectionContext.Provider value={controller}>
      {children}
    </InspectionContext.Provider>
  );
}

export function useInspectionStore<T>(
  selector: (state: InspectionStore) => T,
): T {
  const controller = useController();
  return useStore(controller.store, selector);
}

export function useInspectionDispatch(): (
  command: InspectionCommand,
) => Promise<string> {
  const controller = useController();
  return (command) => controller.dispatch(command);
}

export function useInspectionCommand(): (
  command: InspectionCommand,
) => Promise<CommandResult> {
  const controller = useController();
  return (command) => controller.execute(command);
}

export function useInspectionController(): InspectionController {
  return useController();
}

function useController(): InspectionController {
  const controller = useContext(InspectionContext);
  if (controller === null) {
    throw new Error("InspectionProvider is missing");
  }
  return controller;
}
