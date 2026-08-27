export type SettingsDraftField<T extends object> = Extract<keyof T, string>;

export type SettingsDraftComparators<T extends object> = {
  readonly [K in SettingsDraftField<T>]: (left: T[K], right: T[K]) => boolean;
};

export interface SettingsDraftSubmission<T extends object> {
  readonly values: Partial<T>;
  readonly editSerials: Partial<Record<SettingsDraftField<T>, number>>;
  readonly awaitingFields: readonly SettingsDraftField<T>[];
  readonly acceptedRevision: string | null;
}

export interface SettingsDraftState<T extends object> {
  readonly resourceKey: string | null;
  readonly authority: T | null;
  readonly authorityRevision: string | null;
  readonly overlays: Partial<T>;
  readonly editBases: Partial<T>;
  readonly editSerials: Partial<Record<SettingsDraftField<T>, number>>;
  readonly submission: SettingsDraftSubmission<T> | null;
  readonly conflicts: readonly SettingsDraftField<T>[];
  readonly failure: string | null;
  readonly nextEditSerial: number;
}

export type SettingsDraftEvent<T extends object> =
  | {
      readonly type: "authority";
      readonly resourceKey: string;
      readonly revision: string;
      readonly value: T;
    }
  | {
      readonly type: "edit";
      readonly field: SettingsDraftField<T>;
      readonly value: T[SettingsDraftField<T>];
    }
  | { readonly type: "submit" }
  | { readonly type: "accept"; readonly revision: string }
  | { readonly type: "fail"; readonly message: string }
  | { readonly type: "discard" }
  | { readonly type: "remove"; readonly resourceKey: string };

export type SettingsDraftPhase =
  | "pristine"
  | "dirty"
  | "submitting"
  | "awaiting_view"
  | "failed"
  | "conflict";

export function emptySettingsDraft<T extends object>(): SettingsDraftState<T> {
  return {
    resourceKey: null,
    authority: null,
    authorityRevision: null,
    overlays: {},
    editBases: {},
    editSerials: {},
    submission: null,
    conflicts: [],
    failure: null,
    nextEditSerial: 1,
  };
}

export function initializeSettingsDraft<T extends object>(
  resourceKey: string,
  revision: string,
  value: T,
): SettingsDraftState<T> {
  return {
    ...emptySettingsDraft<T>(),
    resourceKey,
    authority: value,
    authorityRevision: revision,
  };
}

export function reduceSettingsDraft<T extends object>(
  state: SettingsDraftState<T>,
  event: SettingsDraftEvent<T>,
  comparators: SettingsDraftComparators<T>,
): SettingsDraftState<T> {
  switch (event.type) {
    case "authority":
      if (!isCanonicalRevision(event.revision)) {
        return { ...state, failure: "Authoritative settings revision is invalid." };
      }
      if (state.resourceKey !== event.resourceKey || state.authority === null) {
        return initializeSettingsDraft(
          event.resourceKey,
          event.revision,
          event.value,
        );
      }
      if (
        state.authorityRevision !== null &&
        compareRevisions(event.revision, state.authorityRevision) < 0
      ) {
        return state;
      }
      if (
        event.revision === state.authorityRevision &&
        sameSettingsValue(state.authority, event.value, comparators)
      ) {
        return state;
      }
      return applyAuthority(state, event.revision, event.value, comparators);
    case "edit":
      return applyEdit(state, event.field, event.value, comparators);
    case "submit": {
      if (state.submission !== null || !hasOwnFields(state.overlays)) return state;
      const fields = Object.keys(state.overlays) as SettingsDraftField<T>[];
      return {
        ...state,
        submission: {
          values: { ...state.overlays },
          editSerials: { ...state.editSerials },
          awaitingFields: fields,
          acceptedRevision: null,
        },
        failure: null,
      };
    }
    case "accept": {
      if (state.submission === null) return state;
      if (!isCanonicalRevision(event.revision)) {
        return {
          ...state,
          submission: null,
          failure: "Settings receipt revision is invalid.",
        };
      }
      const accepted = {
        ...state,
        submission: {
          ...state.submission,
          acceptedRevision: event.revision,
        },
        failure: null,
      };
      if (accepted.authority === null || accepted.authorityRevision === null) {
        return accepted;
      }
      return applyAuthority(
        accepted,
        accepted.authorityRevision,
        accepted.authority,
        comparators,
      );
    }
    case "fail":
      return {
        ...state,
        submission: null,
        failure: boundedMessage(event.message),
      };
    case "discard":
      return state.authority === null || state.resourceKey === null || state.authorityRevision === null
        ? emptySettingsDraft<T>()
        : initializeSettingsDraft(
            state.resourceKey,
            state.authorityRevision,
            state.authority,
          );
    case "remove":
      return state.resourceKey === event.resourceKey ? emptySettingsDraft<T>() : state;
  }
}

export function settingsDraftValue<T extends object>(
  state: SettingsDraftState<T>,
): T | null {
  return state.authority === null
    ? null
    : ({ ...state.authority, ...state.overlays } as T);
}

export function settingsDraftPhase<T extends object>(
  state: SettingsDraftState<T>,
): SettingsDraftPhase {
  if (state.submission?.acceptedRevision !== null && state.submission !== null) {
    return "awaiting_view";
  }
  if (state.submission !== null) return "submitting";
  if (state.conflicts.length > 0) return "conflict";
  if (state.failure !== null) return "failed";
  return hasOwnFields(state.overlays) ? "dirty" : "pristine";
}

export function settingsDraftFields<T extends object>(
  state: SettingsDraftState<T>,
): readonly SettingsDraftField<T>[] {
  return Object.keys(state.overlays) as SettingsDraftField<T>[];
}

export function compareRevisions(left: string, right: string): -1 | 0 | 1 {
  if (!isCanonicalRevision(left) || !isCanonicalRevision(right)) {
    throw new Error("revision must be a canonical unsigned decimal string");
  }
  if (left.length !== right.length) return left.length < right.length ? -1 : 1;
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function applyEdit<T extends object>(
  state: SettingsDraftState<T>,
  field: SettingsDraftField<T>,
  value: T[SettingsDraftField<T>],
  comparators: SettingsDraftComparators<T>,
): SettingsDraftState<T> {
  if (state.authority === null) return state;
  const equal = comparators[field] as (left: unknown, right: unknown) => boolean;
  const authorityValue = state.authority[field];
  const submittedValue = state.submission?.values[field];
  const protectsNewerEdit =
    submittedValue !== undefined && !equal(value, submittedValue);
  const overlays = { ...state.overlays };
  const editBases = { ...state.editBases };
  const editSerials = { ...state.editSerials };
  let conflicts = state.conflicts;

  if (equal(value, authorityValue) && !protectsNewerEdit) {
    delete overlays[field];
    delete editBases[field];
    delete editSerials[field];
    conflicts = conflicts.filter((candidate) => candidate !== field);
  } else {
    if (!Object.prototype.hasOwnProperty.call(overlays, field)) {
      editBases[field] = authorityValue;
    }
    overlays[field] = value;
    editSerials[field] = state.nextEditSerial;
  }
  return {
    ...state,
    overlays,
    editBases,
    editSerials,
    conflicts,
    failure: null,
    nextEditSerial: state.nextEditSerial + 1,
  };
}

function applyAuthority<T extends object>(
  state: SettingsDraftState<T>,
  revision: string,
  value: T,
  comparators: SettingsDraftComparators<T>,
): SettingsDraftState<T> {
  const overlays = { ...state.overlays };
  const editBases = { ...state.editBases };
  const editSerials = { ...state.editSerials };
  const conflicts = new Set(state.conflicts);
  const submission = state.submission;
  const awaiting = new Set(submission?.awaitingFields ?? []);
  const acceptedRevision = submission?.acceptedRevision ?? null;
  const isSufficientlyNew =
    acceptedRevision !== null && compareRevisions(revision, acceptedRevision) >= 0;

  for (const field of Object.keys(value) as SettingsDraftField<T>[]) {
    if (!Object.prototype.hasOwnProperty.call(overlays, field)) continue;
    const equal = comparators[field] as (left: unknown, right: unknown) => boolean;
    const submittedValue = submission?.values[field];
    const confirmsSubmission =
      isSufficientlyNew &&
      awaiting.has(field) &&
      submittedValue !== undefined &&
      equal(value[field], submittedValue);

    if (confirmsSubmission) {
      awaiting.delete(field);
      const capturedSerial = submission?.editSerials[field];
      if (
        capturedSerial !== undefined &&
        editSerials[field] === capturedSerial &&
        equal(overlays[field], submittedValue)
      ) {
        delete overlays[field];
        delete editBases[field];
        delete editSerials[field];
      } else {
        editBases[field] = value[field];
      }
      conflicts.delete(field);
      continue;
    }

    if (isSufficientlyNew && awaiting.has(field)) {
      awaiting.delete(field);
      conflicts.add(field);
      continue;
    }
    const base = editBases[field];
    if (base !== undefined && !equal(value[field], base)) {
      conflicts.add(field);
    }
  }

  const nextSubmission =
    submission === null
      ? null
      : awaiting.size === 0
        ? null
        : { ...submission, awaitingFields: [...awaiting] };
  return {
    ...state,
    authority: value,
    authorityRevision: revision,
    overlays,
    editBases,
    editSerials,
    submission: nextSubmission,
    conflicts: [...conflicts],
  };
}

function isCanonicalRevision(value: string): boolean {
  return value === "0" || /^[1-9][0-9]*$/.test(value);
}

function hasOwnFields(value: object): boolean {
  return Object.keys(value).length > 0;
}

function sameSettingsValue<T extends object>(
  left: T,
  right: T,
  comparators: SettingsDraftComparators<T>,
): boolean {
  return (Object.keys(right) as SettingsDraftField<T>[]).every((field) =>
    (comparators[field] as (left: unknown, right: unknown) => boolean)(
      left[field],
      right[field],
    ),
  );
}

function boundedMessage(message: string): string {
  const normalized = message.trim() || "Settings update failed.";
  return normalized.slice(0, 512);
}
