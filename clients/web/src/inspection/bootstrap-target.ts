export type InspectionBootstrapTarget =
  | { readonly type: "demo"; readonly parameters: URLSearchParams }
  | { readonly type: "live"; readonly parameters: URLSearchParams }
  | { readonly type: "tauri" };

export function resolveInspectionBootstrapTarget(
  parameters: URLSearchParams,
  hasTauri: boolean,
  defaultLive: string | undefined,
): InspectionBootstrapTarget {
  if (parameters.has("demo")) return { type: "demo", parameters };
  if (hasTauri) return { type: "tauri" };
  if (defaultLive === "same-origin") {
    return { type: "live", parameters };
  }
  return { type: "demo", parameters };
}
