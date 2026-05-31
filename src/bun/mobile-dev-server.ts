import { networkInterfaces } from "node:os";
import type { LoopPreset } from "../shared/app-rpc";
import type {
  MobileConnectionCode,
  MobilePushEnvironment,
  MobilePushRegistrationRequest,
  MobileSessionDetail,
} from "../shared/mobile-contract";
import {
  createFallbackMobileState,
  mapLoopSessionToDetail,
  mapLoopndrollSnapshotToMobile,
} from "./mobile-mappers";
import { registerMobilePushDevice, sendTestPushToInstallation } from "./mobile-push";
import {
  deleteSession,
  getLoopndrollSnapshot,
  saveDefaultPrompt,
  setSessionArchived,
  setSessionPreset,
} from "./loopndroll";

const DEFAULT_MOBILE_DEV_SERVER_HOST = "0.0.0.0";
const DEFAULT_MOBILE_DEV_SERVER_PORT = 8787;
const DEFAULT_PROMPT_FALLBACK = "Keep working on the task. Do not finish yet.";
const EXPLICIT_PUBLIC_BASE_URLS = [
  process.env.LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URLS,
  process.env.LOOPNDROLL_MOBILE_DEV_SERVER_PUBLIC_BASE_URLS,
  process.env.LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URL,
  process.env.LOOPNDROLL_MOBILE_DEV_SERVER_PUBLIC_BASE_URL,
];
const MOBILE_DEV_SERVER_HOST =
  process.env.LOOPER_MOBILE_DEV_SERVER_HOST?.trim() ||
  process.env.LOOPNDROLL_MOBILE_DEV_SERVER_HOST?.trim() ||
  DEFAULT_MOBILE_DEV_SERVER_HOST;
const MOBILE_DEV_SERVER_PORT = Number(
  process.env.LOOPER_MOBILE_DEV_SERVER_PORT?.trim() ||
    process.env.LOOPNDROLL_MOBILE_DEV_SERVER_PORT?.trim() ||
    DEFAULT_MOBILE_DEV_SERVER_PORT,
);

function splitBaseURLValues(value: string) {
  return value
    .split(/[\n,; ]+/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function normalizeBaseURL(value: string) {
  const trimmedValue = value.trim();
  if (!trimmedValue) {
    return null;
  }

  const candidateValue = trimmedValue.includes("://") ? trimmedValue : `http://${trimmedValue}`;
  try {
    const url = new URL(candidateValue);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null;
    }

    return url.toString().replace(/\/$/, "");
  } catch {
    return null;
  }
}

function uniqueBaseURLs(baseURLs: string[]) {
  return Array.from(new Set(baseURLs));
}

function networkIPv4AddressCandidates() {
  const interfaces = networkInterfaces();
  const preferredInterfaceNames = ["en", "utun", "bridge"];
  const candidates = Object.entries(interfaces).flatMap(([name, addresses]) =>
    (addresses ?? [])
      .filter((address) => address.family === "IPv4" && !address.internal)
      .filter((address) => !address.address.startsWith("169.254."))
      .map((address) => ({ name, address: address.address })),
  );

  return preferredInterfaceNames.flatMap((prefix) =>
    candidates.filter((candidate) => candidate.name.startsWith(prefix)),
  );
}

function explicitPublicBaseURLs() {
  return EXPLICIT_PUBLIC_BASE_URLS.flatMap((value) =>
    value
      ? splitBaseURLValues(value)
          .map(normalizeBaseURL)
          .filter((url) => url !== null)
      : [],
  );
}

function explicitPublicHost() {
  return normalizeBaseURL(
    process.env.LOOPER_MOBILE_DEV_SERVER_PUBLIC_HOST?.trim() ||
      process.env.LOOPNDROLL_MOBILE_DEV_SERVER_PUBLIC_HOST?.trim() ||
      "",
  );
}

function advertisedHosts() {
  const publicHost = explicitPublicHost();
  if (publicHost) {
    return [new URL(publicHost).hostname];
  }

  if (MOBILE_DEV_SERVER_HOST === "127.0.0.1") {
    return ["127.0.0.1"];
  }

  const networkAddresses = networkIPv4AddressCandidates().map((candidate) => candidate.address);
  return networkAddresses.length > 0 ? networkAddresses : ["127.0.0.1"];
}

function advertisedBaseURLs() {
  const portfulBaseURLs = advertisedHosts().map(
    (host) => `http://${host}:${MOBILE_DEV_SERVER_PORT}`,
  );

  return uniqueBaseURLs([...explicitPublicBaseURLs(), ...portfulBaseURLs]);
}

function advertisedBaseURL() {
  return advertisedBaseURLs()[0] ?? `http://127.0.0.1:${MOBILE_DEV_SERVER_PORT}`;
}

function encodeBase64URL(value: string) {
  return Buffer.from(value, "utf8")
    .toString("base64")
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}

function createConnectionCode(): MobileConnectionCode {
  const baseURLs = advertisedBaseURLs();
  const baseURL = baseURLs[0] ?? advertisedBaseURL();
  return {
    baseURL,
    baseURLs,
    code: encodeBase64URL(JSON.stringify({ baseURL, baseURLs })),
    generatedAt: new Date().toISOString(),
  };
}

let fallbackState = createFallbackMobileState(advertisedBaseURL());

function jsonResponse(payload: unknown, init?: ResponseInit) {
  const headers = new Headers(init?.headers);
  headers.set("Access-Control-Allow-Origin", "*");
  headers.set("Access-Control-Allow-Headers", "Content-Type");
  headers.set("Access-Control-Allow-Methods", "GET,POST,DELETE,OPTIONS");

  return Response.json(payload, {
    ...init,
    headers,
  });
}

function errorResponse(message: string, status = 400) {
  return jsonResponse({ message }, { status });
}

async function readRequestBody(request: Request) {
  try {
    return (await request.json()) as Record<string, unknown>;
  } catch {
    return {};
  }
}

async function resolveState() {
  const snapshot = await getLoopndrollSnapshot();
  if (snapshot.sessions.length > 0) {
    return {
      kind: "real" as const,
      snapshot,
    };
  }

  return {
    kind: "fallback" as const,
    state: fallbackState,
  };
}

async function handleSnapshotRequest() {
  const state = await resolveState();
  return jsonResponse(
    state.kind === "real"
      ? mapLoopndrollSnapshotToMobile(state.snapshot, advertisedBaseURL())
      : state.state.snapshot,
  );
}

function findSessionDetail(
  state: Awaited<ReturnType<typeof resolveState>>,
  sessionId: string,
): MobileSessionDetail | null {
  if (state.kind === "real") {
    const session = state.snapshot.sessions.find((current) => current.sessionId === sessionId);
    return session ? mapLoopSessionToDetail(session, state.snapshot) : null;
  }

  return state.state.sessionDetails[sessionId] ?? null;
}

async function handleDetailRequest(sessionId: string) {
  const state = await resolveState();
  const detail = findSessionDetail(state, sessionId);
  return detail ? jsonResponse(detail) : errorResponse("Session not found.", 404);
}

function applyFallbackMode(sessionId: string, preset: LoopPreset | null) {
  fallbackState = {
    ...fallbackState,
    snapshot: {
      ...fallbackState.snapshot,
      sessions: fallbackState.snapshot.sessions.map((session) =>
        session.id === sessionId
          ? {
              ...session,
              effectiveMode: preset,
              lastUpdatedAt: new Date().toISOString(),
            }
          : session,
      ),
    },
    sessionDetails: Object.fromEntries(
      Object.entries(fallbackState.sessionDetails).map(([id, session]) => [
        id,
        id === sessionId
          ? {
              ...session,
              effectiveMode: preset,
              lastUpdatedAt: new Date().toISOString(),
            }
          : session,
      ]),
    ),
  };

  return fallbackState.snapshot;
}

async function handleModeRequest(request: Request, sessionId: string) {
  const body = await readRequestBody(request);
  const presetValue = body.preset;
  const preset =
    presetValue == null || typeof presetValue === "string"
      ? (presetValue as LoopPreset | null)
      : null;
  const state = await resolveState();

  if (state.kind === "real") {
    const nextSnapshot = await setSessionPreset(sessionId, preset);
    return jsonResponse(mapLoopndrollSnapshotToMobile(nextSnapshot, advertisedBaseURL()));
  }

  return jsonResponse(applyFallbackMode(sessionId, preset));
}

function applyFallbackArchive(sessionId: string, archived: boolean) {
  fallbackState = {
    ...fallbackState,
    snapshot: {
      ...fallbackState.snapshot,
      sessions: fallbackState.snapshot.sessions.map((session) =>
        session.id === sessionId
          ? {
              ...session,
              isArchived: archived,
              status: archived ? "archived" : "active",
              lastUpdatedAt: new Date().toISOString(),
            }
          : session,
      ),
    },
    sessionDetails: Object.fromEntries(
      Object.entries(fallbackState.sessionDetails).map(([id, session]) => [
        id,
        id === sessionId
          ? {
              ...session,
              isArchived: archived,
              status: archived ? "archived" : "active",
              lastUpdatedAt: new Date().toISOString(),
            }
          : session,
      ]),
    ),
  };

  return fallbackState.snapshot;
}

async function handleArchiveRequest(request: Request, sessionId: string) {
  const body = await readRequestBody(request);
  const archived = Boolean(body.archived);
  const state = await resolveState();

  if (state.kind === "real") {
    const nextSnapshot = await setSessionArchived(sessionId, archived);
    return jsonResponse(mapLoopndrollSnapshotToMobile(nextSnapshot, advertisedBaseURL()));
  }

  return jsonResponse(applyFallbackArchive(sessionId, archived));
}

function applyFallbackDelete(sessionId: string) {
  fallbackState = {
    ...fallbackState,
    snapshot: {
      ...fallbackState.snapshot,
      sessions: fallbackState.snapshot.sessions.filter((session) => session.id !== sessionId),
    },
    sessionDetails: Object.fromEntries(
      Object.entries(fallbackState.sessionDetails).filter(([id]) => id !== sessionId),
    ),
  };

  return fallbackState.snapshot;
}

async function handleDeleteRequest(sessionId: string) {
  const state = await resolveState();

  if (state.kind === "real") {
    const nextSnapshot = await deleteSession(sessionId);
    return jsonResponse(mapLoopndrollSnapshotToMobile(nextSnapshot, advertisedBaseURL()));
  }

  return jsonResponse(applyFallbackDelete(sessionId));
}

function applyFallbackPrompt(defaultPrompt: string) {
  fallbackState = {
    ...fallbackState,
    snapshot: {
      ...fallbackState.snapshot,
      globalSettings: {
        ...fallbackState.snapshot.globalSettings,
        defaultPrompt,
      },
    },
  };

  return fallbackState.snapshot;
}

async function handleDefaultPromptRequest(request: Request) {
  const body = await readRequestBody(request);
  const defaultPrompt =
    typeof body.defaultPrompt === "string" ? body.defaultPrompt.trim() : DEFAULT_PROMPT_FALLBACK;
  const state = await resolveState();

  if (state.kind === "real") {
    const nextSnapshot = await saveDefaultPrompt(defaultPrompt);
    return jsonResponse(mapLoopndrollSnapshotToMobile(nextSnapshot, advertisedBaseURL()));
  }

  return jsonResponse(applyFallbackPrompt(defaultPrompt));
}

function parseMobilePushEnvironment(value: unknown): MobilePushEnvironment | null {
  return value === "development" || value === "production" ? value : null;
}

async function handlePushRegistrationRequest(request: Request) {
  const body = await readRequestBody(request);
  const environment = parseMobilePushEnvironment(body.environment);
  const deviceToken = typeof body.deviceToken === "string" ? body.deviceToken.trim() : "";
  const bundleId = typeof body.bundleId === "string" ? body.bundleId.trim() : "";
  const installationId = typeof body.installationId === "string" ? body.installationId.trim() : "";
  const deviceName = typeof body.deviceName === "string" ? body.deviceName.trim() : null;

  if (!environment || !deviceToken || !bundleId || !installationId) {
    return errorResponse("Push registration request is missing required values.");
  }

  const payload: MobilePushRegistrationRequest = {
    installationId,
    deviceToken,
    bundleId,
    environment,
    deviceName,
  };

  try {
    return jsonResponse(await registerMobilePushDevice(payload));
  } catch (error) {
    return errorResponse(error instanceof Error ? error.message : "Push registration failed.", 500);
  }
}

async function handlePushTestRequest(request: Request) {
  const body = await readRequestBody(request);
  const installationId = typeof body.installationId === "string" ? body.installationId.trim() : "";
  if (!installationId) {
    return errorResponse("A push test requires an installation ID.");
  }

  try {
    return jsonResponse(await sendTestPushToInstallation(installationId));
  } catch (error) {
    return errorResponse(error instanceof Error ? error.message : "Push test failed.", 500);
  }
}

async function routeRequest(request: Request) {
  const url = new URL(request.url);
  const pathSegments = url.pathname.split("/").filter(Boolean);

  if (request.method === "GET" && url.pathname === "/api/mobile/snapshot") {
    return handleSnapshotRequest();
  }

  if (request.method === "GET" && url.pathname === "/api/mobile/health") {
    return jsonResponse({
      ok: true,
      baseURL: advertisedBaseURL(),
      baseURLs: advertisedBaseURLs(),
      serverTime: new Date().toISOString(),
    });
  }

  if (request.method === "GET" && url.pathname === "/api/mobile/connection-code") {
    return jsonResponse(createConnectionCode());
  }

  if (
    request.method === "GET" &&
    pathSegments.length === 4 &&
    pathSegments[0] === "api" &&
    pathSegments[1] === "mobile" &&
    pathSegments[2] === "sessions"
  ) {
    return handleDetailRequest(pathSegments[3] ?? "");
  }

  if (
    request.method === "POST" &&
    pathSegments.length === 5 &&
    pathSegments[0] === "api" &&
    pathSegments[1] === "mobile" &&
    pathSegments[2] === "sessions" &&
    pathSegments[4] === "mode"
  ) {
    return handleModeRequest(request, pathSegments[3] ?? "");
  }

  if (
    request.method === "POST" &&
    pathSegments.length === 5 &&
    pathSegments[0] === "api" &&
    pathSegments[1] === "mobile" &&
    pathSegments[2] === "sessions" &&
    pathSegments[4] === "archive"
  ) {
    return handleArchiveRequest(request, pathSegments[3] ?? "");
  }

  if (
    request.method === "DELETE" &&
    pathSegments.length === 4 &&
    pathSegments[0] === "api" &&
    pathSegments[1] === "mobile" &&
    pathSegments[2] === "sessions"
  ) {
    return handleDeleteRequest(pathSegments[3] ?? "");
  }

  if (request.method === "POST" && url.pathname === "/api/mobile/settings/default-prompt") {
    return handleDefaultPromptRequest(request);
  }

  if (request.method === "POST" && url.pathname === "/api/mobile/push/register") {
    return handlePushRegistrationRequest(request);
  }

  if (request.method === "POST" && url.pathname === "/api/mobile/push/test") {
    return handlePushTestRequest(request);
  }

  return errorResponse("Route not found.", 404);
}

const server = Bun.serve({
  hostname: MOBILE_DEV_SERVER_HOST,
  port: MOBILE_DEV_SERVER_PORT,
  async fetch(request) {
    if (request.method === "OPTIONS") {
      return jsonResponse({ ok: true });
    }

    return routeRequest(request);
  },
});

console.log(`looper mobile dev API listening at http://${MOBILE_DEV_SERVER_HOST}:${server.port}`);
console.log(`looper mobile iPhone URLs: ${advertisedBaseURLs().join(", ")}`);
console.log(`looper mobile device code: ${createConnectionCode().code}`);
