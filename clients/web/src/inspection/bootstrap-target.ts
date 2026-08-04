export type InspectionBootstrapTarget =
  | { readonly type: "demo"; readonly parameters: URLSearchParams }
  | { readonly type: "live"; readonly parameters: URLSearchParams }
  | { readonly type: "tauri" };

export function resolveInspectionBootstrapTarget(
  parameters: URLSearchParams,
  hasTauri: boolean,
  defaultLive: string | undefined,
  pageOrigin: string,
): InspectionBootstrapTarget {
  if (parameters.has("demo")) return { type: "demo", parameters };
  if (parameters.has("live")) return { type: "live", parameters };
  if (hasTauri) return { type: "tauri" };
  if (defaultLive === "same-origin") {
    const liveParameters = new URLSearchParams(parameters);
    liveParameters.set("live", pageOrigin);
    return { type: "live", parameters: liveParameters };
  }
  return { type: "demo", parameters };
}
