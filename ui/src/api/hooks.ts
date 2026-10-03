import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, getEndpoint } from "./client";
import type {
  Action,
  ActionOutcome,
  ActivityItem,
  Anchor,
  Camera,
  CameraAvailable,
  CameraStatus,
  CaptureSession,
  ClassifierReport,
  EngineStatus,
  Gesture,
  GesturePackPreview,
  HaArea,
  HaClientCertificate,
  HaClientCertificateUpload,
  HaConnectionUpdate,
  HaDiscovery,
  HaEntity,
  HaInstance,
  HaStatus,
  Health,
  Mapping,
  ModelPack,
  MotionTake,
  Place,
  PreviewTicket,
  RealignSession,
  SettingsMap,
  SettingsPatch,
  TeachCommitResponse,
  TeachLevelResponse,
  TeachSession,
  TeachSpotResponse,
  TeachVerb,
  UpdateState,
} from "./types";

export const queryKeys = {
  health: ["health"] as const,
  status: ["status"] as const,
  settings: ["settings"] as const,
  haDiscover: ["ha", "discover"] as const,
  haStatus: ["ha", "status"] as const,
  haClientCertificate: ["ha", "client-certificate"] as const,
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
    queryFn: () => api.get<HaStatus>("/api/v1/ha/status"),
  });
}

export function useHaConnect() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { base_url: string; token: string; trust_cert_sha256?: string }) =>
      api.post<HaInstance>("/api/v1/ha/connect", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.haStatus }),
  });
}

export function useHaUpdate() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (update: HaConnectionUpdate) => api.patch<HaInstance>("/api/v1/ha", update),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.haStatus }),
  });
}

export function useHaClientCertificate() {
  return useQuery({
    queryKey: queryKeys.haClientCertificate,
    queryFn: () => api.get<HaClientCertificate>("/api/v1/ha/client-certificate"),
  });
}

export function useHaClientCertificateImport() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (upload: HaClientCertificateUpload) =>
      api.put<HaClientCertificate>("/api/v1/ha/client-certificate", upload),
    onSuccess: (certificate) => {
      client.setQueryData(queryKeys.haClientCertificate, certificate);
      void client.invalidateQueries({ queryKey: queryKeys.haStatus });
    },
  });
}

export function useHaClientCertificateRemove() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.delete<void>("/api/v1/ha/client-certificate"),
    onSuccess: () => {
      client.setQueryData<HaClientCertificate>(queryKeys.haClientCertificate, { installed: false });
      void client.invalidateQueries({ queryKey: queryKeys.haStatus });
    },
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

export function useCreateCamera() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: {
      name: string;
      kind: string;
      device_ref?: string | null;
      url_redacted?: string | null;
    }) => api.post<Camera>("/api/v1/cameras", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.cameras }),
  });
}

export function useStartCamera() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.post<CameraStatus>(`/api/v1/cameras/${id}/start`),
    onSuccess: (camera) => {
      client.setQueryData<EngineStatus>(queryKeys.status, (status) =>
        status
          ? {
              ...status,
              cameras: status.cameras.map((item) =>
                item.camera_id === camera.camera_id ? { ...item, ...camera } : item,
              ),
            }
          : status,
      );
      client.invalidateQueries({ queryKey: queryKeys.status });
    },
  });
}

export function useStopCamera() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.post<CameraStatus>(`/api/v1/cameras/${id}/stop`),
    onSuccess: (camera) => {
      client.setQueryData<EngineStatus>(queryKeys.status, (status) =>
        status
          ? {
              ...status,
              cameras: status.cameras.map((item) =>
                item.camera_id === camera.camera_id ? { ...item, ...camera } : item,
              ),
            }
          : status,
      );
      client.invalidateQueries({ queryKey: queryKeys.status });
    },
  });
}

/**
 * Mints a stream URL for one preview connection. Tickets are single-use on the engine, so
 * this must run per connection and never be cached — a reused URL is rejected with 401.
 * Resolves to null in mock mode, where there is no stream to show.
 */
export async function requestPreviewUrl(cameraId: string): Promise<string | null> {
  const [ticket, endpoint] = await Promise.all([
    api.post<PreviewTicket>(`/api/v1/cameras/${cameraId}/preview-ticket`),
    getEndpoint(),
  ]);
  return endpoint.mock ? null : new URL(ticket.url, `${endpoint.baseUrl}/`).toString();
}

export function useGestures() {
  return useQuery({ queryKey: queryKeys.gestures, queryFn: () => api.get<Gesture[]>("/api/v1/gestures") });
}

export function useCreateGesture() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: {
      name: string;
      kind?: "static" | "motion";
      hands_required?: number;
      hand_constraint: string;
      icon?: string;
      threshold?: number;
    }) => api.post<Gesture>("/api/v1/gestures", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.gestures }),
  });
}

export function usePatchGesture() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: Partial<Gesture> }) =>
      api.patch<Gesture>(`/api/v1/gestures/${id}`, patch),
    onSuccess: (updated) => {
      client.setQueryData<Gesture[]>(queryKeys.gestures, (gestures) =>
        gestures?.map((gesture) => (gesture.id === updated.id ? { ...gesture, ...updated } : gesture)),
      );
    },
  });
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

export function usePatchMapping() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: Partial<Mapping> }) =>
      api.patch<Mapping>(`/api/v1/mappings/${id}`, patch),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.mappings }),
  });
}

export function useDeleteMapping() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.delete<void>(`/api/v1/mappings/${id}`),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.mappings }),
  });
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

export function usePatchAnchor() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      patch,
    }: {
      id: string;
      patch: { name?: string; verb_params?: Record<string, unknown> };
    }) => api.patch<Anchor>(`/api/v1/anchors/${id}`, patch),
    onSuccess: () => client.invalidateQueries({ queryKey: ["anchors"] }),
  });
}

export function useDeleteAnchor() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.delete<void>(`/api/v1/anchors/${id}`),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ["anchors"] });
      client.invalidateQueries({ queryKey: queryKeys.mappings });
    },
  });
}

export function useStartTeach() {
  return useMutation({
    mutationFn: (body: {
      camera_id: string;
      target: Record<string, string>;
      anchor_id?: string;
      append?: boolean;
    }) => api.post<TeachSession>("/api/v1/teach", body),
  });
}

export function useTeachSpot(sessionId?: string) {
  return useMutation({
    mutationFn: () => api.post<TeachSpotResponse>(`/api/v1/teach/${sessionId}/spot`),
  });
}

export function useTeachCurrentLevel(sessionId?: string) {
  return useMutation({
    mutationFn: (body: { level: number }) =>
      api.post<TeachLevelResponse>(`/api/v1/teach/${sessionId}/levels/use-current`, body),
  });
}

export function useTeachLevelTest(sessionId?: string) {
  return useMutation({
    mutationFn: (body: { level: number }) =>
      api.post<ActionOutcome>(`/api/v1/teach/${sessionId}/levels/test`, body),
  });
}

export function useCommitTeach(sessionId?: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { name?: string; verbs: TeachVerb[] }) =>
      api.post<TeachCommitResponse>(`/api/v1/teach/${sessionId}/commit`, body),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ["anchors"] });
      client.invalidateQueries({ queryKey: queryKeys.mappings });
    },
  });
}

export function useCancelTeach(sessionId?: string) {
  return useMutation({
    mutationFn: () => api.post<void>(`/api/v1/teach/${sessionId}/cancel`),
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

export function useInstallUpdate() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { kind: "app" | "pack"; id?: string }) =>
      api.post<void>("/api/v1/updates/install", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.updates }),
  });
}

export function useRollbackUpdate() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { kind: "app" | "pack"; id?: string }) =>
      api.post<void>("/api/v1/updates/rollback", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.updates }),
  });
}

export function useExportGesturePack() {
  return useMutation({
    mutationFn: (body: { gesture_ids: string[]; include_mapping_templates: boolean }) =>
      api.post<Record<string, unknown>>("/api/v1/packs/export", body),
  });
}

export function usePreviewGesturePack() {
  return useMutation({
    mutationFn: (pack: Record<string, unknown>) =>
      api.post<GesturePackPreview>("/api/v1/packs/import/preview", pack),
  });
}

export function useCommitGesturePack() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { pack: Record<string, unknown>; choices: Record<string, unknown> }) =>
      api.post<{ imported_gesture_ids: string[] }>("/api/v1/packs/import/commit", body),
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.gestures }),
  });
}

export function useModels() {
  return useQuery({ queryKey: queryKeys.models, queryFn: () => api.get<ModelPack[]>("/api/v1/models") });
}
