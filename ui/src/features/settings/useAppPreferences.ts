import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type AppPreferences, getAppPreferences, isTauri, setAppPreferences } from "../../platform/tauri";

const queryKey = ["app-preferences"];

// Browser and mock builds have no shell; keep the defaults in memory so the UI stays usable.
let browserPreferences: AppPreferences = { menu_bar: true, open_at_login: false };

export function useAppPreferences() {
  return useQuery({
    queryKey,
    queryFn: () => (isTauri() ? getAppPreferences() : Promise.resolve(browserPreferences)),
  });
}

export function useSetAppPreferences() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (patch: Partial<AppPreferences>) => {
      if (isTauri()) return setAppPreferences(patch);
      browserPreferences = { ...browserPreferences, ...patch };
      return browserPreferences;
    },
    onMutate: async (patch) => {
      await client.cancelQueries({ queryKey });
      const previous = client.getQueryData<AppPreferences>(queryKey);
      if (previous) client.setQueryData(queryKey, { ...previous, ...patch });
      return { previous };
    },
    onError: (_error, _patch, context) => {
      if (context?.previous) client.setQueryData(queryKey, context.previous);
    },
    onSuccess: (preferences) => client.setQueryData(queryKey, preferences),
  });
}
