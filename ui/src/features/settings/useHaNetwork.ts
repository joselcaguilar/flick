import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "../../api/hooks";
import {
  currentWifiSsid,
  isTauri,
  type LocationPermission,
  locationPermissionStatus,
  locationRequestAccess,
} from "../../platform/tauri";

const permissionKey = ["location-permission"];
const ssidKey = ["wifi-ssid"];

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export function useLocationPermission() {
  return useQuery({
    queryKey: permissionKey,
    queryFn: locationPermissionStatus,
    enabled: isTauri(),
    refetchOnWindowFocus: true,
  });
}

export function useRequestLocation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      let status: LocationPermission = await locationRequestAccess();
      // The macOS prompt answers asynchronously; wait for the user's choice.
      for (let attempt = 0; status === "not_determined" && attempt < 60; attempt += 1) {
        await sleep(1000);
        status = await locationPermissionStatus();
      }
      return status;
    },
    onSuccess: (status) => {
      client.setQueryData(permissionKey, status);
      void client.invalidateQueries({ queryKey: ssidKey });
      // The shell reports the new network to the engine within a few seconds.
      setTimeout(() => void client.invalidateQueries({ queryKey: queryKeys.haStatus }), 6000);
    },
  });
}

export function useWifiSsid(enabled: boolean) {
  return useQuery({
    queryKey: ssidKey,
    queryFn: currentWifiSsid,
    enabled: isTauri() && enabled,
    refetchInterval: 5000,
  });
}
