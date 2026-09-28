import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import type {
  Action,
  ActionOutcome,
  ActivityItem,
  Anchor,
  Camera,
  CameraAvailable,
  CaptureSession,
  ClassifierReport,
  EngineStatus,
  Gesture,
  HaArea,
  HaDiscovery,
  HaEntity,
  HaInstance,
  Health,
  Mapping,
  ModelPack,
  MotionTake,
  Place,
  RealignSession,
  SettingsMap,
  SettingsPatch,
  TeachSession,
  UpdateState,
} from "./types";

export const queryKeys = {
  health: ["health"] as const,
  status: ["status"] as const,
  settings: ["settings"] as const,
  haDiscover: ["ha", "discover"] as const,
  haStatus: ["ha", "status"] as const,
  haAreas: ["ha", "areas"] as const,
  haEntities: (params = "") => ["ha", "entities", params] as const,
  haServices: (domain = "") => ["ha", "services", domain] as const,
  cameras: ["cameras"] as const,
  camerasAvailable: ["cameras", "available"] as const,
  gestures: ["gestures"] as const,
  gestureMotionTakes: (id: string) => ["gestures", id, "motion-takes"] as const,
  gestureSamples: (id: string) => ["gestures", id, "samples"] as const,
  classifier: ["classifier"] as const,
  mappings: ["mappings"] as const,
  activity: (params = "") => ["activity", params] as const,
  places: (cameraId = "") => ["places", cameraId] as const,
  anchors: (placeId = "") => ["anchors", placeId] as const,
  updates: ["updates"] as const,
  models: ["models"] as const,
};

export function useHealth() {
  return useQuery({ queryKey: queryKeys.health, queryFn: () => api.get<Health>("/health") });
}

export function useStatus() {
  return useQuery({ queryKey: queryKeys.status, queryFn: () => api.get<EngineStatus>("/api/v1/status") });
}

export function usePauseEngine() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (duration_s?: number) => api.post<EngineStatus>("/api/v1/engine/pause", { duration_s }),
    onSuccess: (status) => client.setQueryData(queryKeys.status, status),
  });
}

export function useResumeEngine() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<EngineStatus>("/api/v1/engine/resume"),
    onSuccess: (status) => client.setQueryData(queryKeys.status, status),
  });
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: () => api.get<SettingsMap>("/api/v1/settings") });
}

export function usePatchSettings() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (patch: SettingsPatch) => api.patch<SettingsMap>("/api/v1/settings", patch),
    onSuccess: (settings) => client.setQueryData(queryKeys.settings, settings),
  });
}

export function useHaDiscover() {
  return useQuery({
    queryKey: queryKeys.haDiscover,
    queryFn: () => api.get<HaDiscovery[]>("/api/v1/ha/discover"),
  });
}

export function useHaStatus() {
  return useQuery({
    queryKey: queryKeys.haStatus,
    queryFn: () =>
      api.get<{ state: string; ha_version?: string; instance?: HaInstance }>("/api/v1/ha/status"),
  });
}

export function useHaConnect() {
  return useMutation({
    mutationFn: (body: { base_url: string; token: string; trust_cert_sha256?: string }) =>
      api.post<HaInstance>("/api/v1/ha/connect", body),
  });
}

export function useHaAreas() {
  return useQuery({ queryKey: queryKeys.haAreas, queryFn: () => api.get<HaArea[]>("/api/v1/ha/areas") });
}

export function useHaEntities(params = "") {
  return useQuery({
    queryKey: queryKeys.haEntities(params),
    queryFn: () => api.get<HaEntity[]>(`/api/v1/ha/entities${params}`),
  });
}

export function useHaCall() {
  return useMutation({ mutationFn: (action: Action) => api.post<ActionOutcome>("/api/v1/ha/call", action) });
}

export function useCamerasAvailable() {
  return useQuery({
    queryKey: queryKeys.camerasAvailable,
    queryFn: () => api.get<CameraAvailable[]>("/api/v1/cameras/available"),
  });
}

export function useCameras() {
  return useQuery({ queryKey: queryKeys.cameras, queryFn: () => api.get<Camera[]>("/api/v1/cameras") });
}

export function useGestures() {
  return useQuery({ queryKey: queryKeys.gestures, queryFn: () => api.get<Gesture[]>("/api/v1/gestures") });
}

export function useGestureMotionTakes(id: string) {
  return useQuery({
    queryKey: queryKeys.gestureMotionTakes(id),
    queryFn: () => api.get<MotionTake[]>(`/api/v1/gestures/${id}/motion-takes`),
  });
}

export function useCaptureGesture(id: string) {
  return useMutation({
    mutationFn: (body: {
      camera_id?: string;
      kind: "positive" | "negative";
      takes: number;
      take_ms: number;
    }) => api.post<CaptureSession>(`/api/v1/gestures/${id}/capture`, body),
  });
}

export function useClassifier() {
  return useQuery({
    queryKey: queryKeys.classifier,
    queryFn: () => api.get<ClassifierReport>("/api/v1/classifier"),
  });
}

export function useTrainClassifier() {
  return useMutation({ mutationFn: () => api.post<ClassifierReport>("/api/v1/classifier/train", {}) });
}

export function useMappings() {
  return useQuery({ queryKey: queryKeys.mappings, queryFn: () => api.get<Mapping[]>("/api/v1/mappings") });
}

export function useTestMapping(id: string) {
  return useMutation({ mutationFn: () => api.post<ActionOutcome>(`/api/v1/mappings/${id}/test`) });
}

export function useActivity(params = "?limit=10") {
  return useQuery({
    queryKey: queryKeys.activity(params),
    queryFn: () => api.get<{ items: ActivityItem[]; next_before?: string }>(`/api/v1/activity${params}`),
  });
}

export function usePlaces(cameraId = "") {
  const suffix = cameraId ? `?camera_id=${encodeURIComponent(cameraId)}` : "";
  return useQuery({
    queryKey: queryKeys.places(cameraId),
    queryFn: () => api.get<Place[]>(`/api/v1/places${suffix}`),
  });
}

export function useAnchors(placeId = "") {
  const suffix = placeId ? `?place_id=${encodeURIComponent(placeId)}` : "";
  return useQuery({
    queryKey: queryKeys.anchors(placeId),
    queryFn: () => api.get<Anchor[]>(`/api/v1/anchors${suffix}`),
  });
}

export function useStartTeach() {
  return useMutation({
    mutationFn: (body: { camera_id: string; target: Record<string, string>; anchor_id?: string }) =>
      api.post<TeachSession>("/api/v1/teach", body),
  });
}

export function useStartRealign(placeId: string) {
  return useMutation({ mutationFn: () => api.post<RealignSession>(`/api/v1/places/${placeId}/realign`) });
}

export function useUpdates() {
  return useQuery({ queryKey: queryKeys.updates, queryFn: () => api.get<UpdateState>("/api/v1/updates") });
}

export function useCheckUpdates() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<UpdateState>("/api/v1/updates/check"),
    onSuccess: (updates) => client.setQueryData(queryKeys.updates, updates),
  });
}

export function useModels() {
  return useQuery({ queryKey: queryKeys.models, queryFn: () => api.get<ModelPack[]>("/api/v1/models") });
}
