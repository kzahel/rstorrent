import { useCallback, useEffect, useState } from "react";

import {
  initializeSettingsDraft,
  reduceSettingsDraft,
  type SettingsDraftComparators,
  type SettingsDraftEvent,
  type SettingsDraftState,
} from "./settings-draft";

export function useSettingsDraft<T extends object>(
  resourceKey: string,
  revision: string,
  authority: T,
  comparators: SettingsDraftComparators<T>,
): readonly [
  SettingsDraftState<T>,
  (event: SettingsDraftEvent<T>) => void,
] {
  const [state, setState] = useState(() =>
    initializeSettingsDraft(resourceKey, revision, authority),
  );

  useEffect(() => {
    setState((current) =>
      reduceSettingsDraft(
        current,
        { type: "authority", resourceKey, revision, value: authority },
        comparators,
      ),
    );
  }, [authority, comparators, resourceKey, revision]);

  const dispatch = useCallback(
    (event: SettingsDraftEvent<T>) => {
      setState((current) => reduceSettingsDraft(current, event, comparators));
    },
    [comparators],
  );
  return [state, dispatch] as const;
}
