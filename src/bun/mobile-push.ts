import { readFile } from "node:fs/promises";
import { connect } from "node:http2";
import { and, eq, or } from "drizzle-orm";
import type {
  MobilePushEnvironment,
  MobilePushRegistrationRequest,
  MobilePushRegistrationResponse,
  MobilePushTestResponse,
} from "../shared/mobile-contract";
import { getLoopndrollDatabase } from "./db/client";
import { mobilePushDevices } from "./db/schema";
import { getLoopndrollPaths, nowIsoString } from "./loopndroll-core";

const APNS_ALERT_PRIORITY = "10";
const APNS_DISABLE_REASONS = new Set(["BadDeviceToken", "DeviceTokenNotForTopic", "Unregistered"]);
const APNS_PUSH_TYPE_ALERT = "alert";
const APNS_SOUND_DEFAULT = "default";
const APNS_SUCCESS_STATUS = 200;
const APNS_TEST_BODY = "Remote notifications are working.";
const APNS_TEST_SUBTITLE = "TestFlight push ready";
const APNS_TEST_TITLE = "Looper";
const APNS_TOPIC_HEADER = "apns-topic";
const APNS_PRIORITY_HEADER = "apns-priority";
const APNS_PUSH_TYPE_HEADER = "apns-push-type";
const AUTHORIZATION_HEADER = "authorization";
const CONTENT_TYPE_HEADER = "content-type";
const JSON_CONTENT_TYPE = "application/json";
const MOBILE_PUSH_ENABLED_STATE = "enabled";
const MOBILE_PUSH_PENDING_STATE = "stored-awaiting-provider";

type ApnsProviderConfigFile = {
  keyId?: unknown;
  teamId?: unknown;
  bundleId?: unknown;
  environment?: unknown;
  privateKeyPath?: unknown;
  privateKeyBase64?: unknown;
  privateKeyPem?: unknown;
};

type LoadedApnsProviderConfig = {
  bundleId: string;
  environment: MobilePushEnvironment;
  keyId: string;
  teamId: string;
  privateKeyPem: string;
};

type ApnsAlertMessage = {
  body: string;
  subtitle: string;
  title: string;
  threadId: string;
  userInfo: Record<string, string>;
};

type StoredPushDevice = typeof mobilePushDevices.$inferSelect;

function trimNonEmptyString(value: unknown) {
  if (typeof value !== "string") {
    return null;
  }

  const trimmedValue = value.trim();
  return trimmedValue.length > 0 ? trimmedValue : null;
}

function normalizeApnsEnvironment(value: unknown): MobilePushEnvironment | null {
  return value === "development" || value === "production" ? value : null;
}

function encodeBase64Url(value: Buffer | Uint8Array | string) {
  const buffer = typeof value === "string" ? Buffer.from(value, "utf8") : Buffer.from(value);
  return buffer.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

function pemToPkcs8(pem: string) {
  const normalizedValue = pem
    .replace(/-----BEGIN PRIVATE KEY-----/g, "")
    .replace(/-----END PRIVATE KEY-----/g, "")
    .replace(/\s+/g, "");

  return Buffer.from(normalizedValue, "base64");
}

async function readApnsConfigFile(apnsConfigPath: string) {
  try {
    return JSON.parse(await readFile(apnsConfigPath, "utf8")) as ApnsProviderConfigFile;
  } catch (error) {
    if ((error as NodeJS.ErrnoException)?.code === "ENOENT") {
      return {};
    }

    throw error;
  }
}

async function loadPrivateKeyPem(fileConfig: ApnsProviderConfigFile, env: NodeJS.ProcessEnv) {
  const inlinePem =
    trimNonEmptyString(env.LOOPER_APNS_PRIVATE_KEY_PEM) ??
    trimNonEmptyString(fileConfig.privateKeyPem);
  if (inlinePem) {
    return inlinePem;
  }

  const base64Pem =
    trimNonEmptyString(env.LOOPER_APNS_PRIVATE_KEY_BASE64) ??
    trimNonEmptyString(fileConfig.privateKeyBase64);
  if (base64Pem) {
    return Buffer.from(base64Pem, "base64").toString("utf8");
  }

  const privateKeyPath =
    trimNonEmptyString(env.LOOPER_APNS_PRIVATE_KEY_PATH) ??
    trimNonEmptyString(fileConfig.privateKeyPath);
  if (!privateKeyPath) {
    return null;
  }

  return (await readFile(privateKeyPath, "utf8")).trim();
}

export async function loadApnsProviderConfigFromSources(
  env: NodeJS.ProcessEnv,
  fileConfig: ApnsProviderConfigFile,
) {
  const keyId = trimNonEmptyString(env.LOOPER_APNS_KEY_ID) ?? trimNonEmptyString(fileConfig.keyId);
  const teamId =
    trimNonEmptyString(env.LOOPER_APNS_TEAM_ID) ?? trimNonEmptyString(fileConfig.teamId);
  const bundleId =
    trimNonEmptyString(env.LOOPER_APNS_BUNDLE_ID) ?? trimNonEmptyString(fileConfig.bundleId);
  const environment =
    normalizeApnsEnvironment(env.LOOPER_APNS_ENVIRONMENT) ??
    normalizeApnsEnvironment(fileConfig.environment) ??
    "production";
  const privateKeyPem = await loadPrivateKeyPem(fileConfig, env);

  if (!keyId || !teamId || !bundleId || !privateKeyPem) {
    return null;
  }

  return {
    bundleId,
    environment,
    keyId,
    teamId,
    privateKeyPem,
  } satisfies LoadedApnsProviderConfig;
}

export async function loadApnsProviderConfig(apnsConfigPath = getLoopndrollPaths().apnsConfigPath) {
  const fileConfig = await readApnsConfigFile(apnsConfigPath);
  return loadApnsProviderConfigFromSources(process.env, fileConfig);
}

export async function createApnsJwt(
  config: LoadedApnsProviderConfig,
  issuedAt = Math.floor(Date.now() / 1000),
) {
  const header = encodeBase64Url(JSON.stringify({ alg: "ES256", kid: config.keyId }));
  const claims = encodeBase64Url(JSON.stringify({ iss: config.teamId, iat: issuedAt }));
  const unsignedToken = `${header}.${claims}`;
  const cryptoKey = await crypto.subtle.importKey(
    "pkcs8",
    pemToPkcs8(config.privateKeyPem),
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign"],
  );
  const signature = new Uint8Array(
    await crypto.subtle.sign(
      { name: "ECDSA", hash: "SHA-256" },
      cryptoKey,
      Buffer.from(unsignedToken, "utf8"),
    ),
  );

  return `${unsignedToken}.${encodeBase64Url(signature)}`;
}

function getApnsOrigin(environment: MobilePushEnvironment) {
  return environment === "production"
    ? "https://api.push.apple.com"
    : "https://api.sandbox.push.apple.com";
}

function buildApnsRequestBody(message: ApnsAlertMessage) {
  return JSON.stringify({
    aps: {
      alert: {
        title: message.title,
        subtitle: message.subtitle,
        body: message.body,
      },
      sound: APNS_SOUND_DEFAULT,
      "thread-id": message.threadId,
    },
    ...message.userInfo,
  });
}

async function sendApnsAlert(
  config: LoadedApnsProviderConfig,
  deviceToken: string,
  message: ApnsAlertMessage,
) {
  const payload = buildApnsRequestBody(message);
  const authorization = `bearer ${await createApnsJwt(config)}`;

  return await new Promise<{
    ok: boolean;
    reason: string | null;
    status: number;
  }>((resolve, reject) => {
    const client = connect(getApnsOrigin(config.environment));
    let settled = false;

    const finish = (result: { ok: boolean; reason: string | null; status: number }) => {
      if (settled) {
        return;
      }

      settled = true;
      client.close();
      resolve(result);
    };

    client.on("error", (error) => {
      if (settled) {
        return;
      }

      settled = true;
      reject(error);
    });

    const request = client.request({
      ":method": "POST",
      ":path": `/3/device/${deviceToken}`,
      [AUTHORIZATION_HEADER]: authorization,
      [CONTENT_TYPE_HEADER]: JSON_CONTENT_TYPE,
      [APNS_PRIORITY_HEADER]: APNS_ALERT_PRIORITY,
      [APNS_PUSH_TYPE_HEADER]: APNS_PUSH_TYPE_ALERT,
      [APNS_TOPIC_HEADER]: config.bundleId,
    });

    const responseChunks: Buffer[] = [];
    let responseStatus = 0;

    request.on("response", (headers) => {
      const statusHeader = headers[":status"];
      responseStatus = typeof statusHeader === "number" ? statusHeader : Number(statusHeader ?? 0);
    });

    request.on("data", (chunk) => {
      responseChunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    });

    request.on("end", () => {
      const responseBody = Buffer.concat(responseChunks).toString("utf8").trim();
      let reason: string | null = null;

      if (responseBody.length > 0) {
        try {
          const parsedResponse = JSON.parse(responseBody) as { reason?: unknown };
          reason = trimNonEmptyString(parsedResponse.reason);
        } catch {
          reason = responseBody;
        }
      }

      finish({
        ok: responseStatus === APNS_SUCCESS_STATUS,
        reason,
        status: responseStatus,
      });
    });

    request.on("error", reject);
    request.end(payload);
  });
}

function isProviderReadyForDevice(
  config: LoadedApnsProviderConfig | null,
  device: Pick<MobilePushRegistrationRequest, "bundleId" | "environment">,
) {
  return (
    config !== null &&
    config.bundleId === device.bundleId &&
    config.environment === device.environment
  );
}

function getDatabase() {
  return getLoopndrollDatabase(getLoopndrollPaths().databasePath).db;
}

export async function registerMobilePushDevice(
  request: MobilePushRegistrationRequest,
): Promise<MobilePushRegistrationResponse> {
  const deviceToken = trimNonEmptyString(request.deviceToken);
  const bundleId = trimNonEmptyString(request.bundleId);
  const installationId = trimNonEmptyString(request.installationId);

  if (!deviceToken || !bundleId || !installationId) {
    throw new Error("Push registration is missing required device information.");
  }

  const timestamp = nowIsoString();
  const db = getDatabase();

  db.delete(mobilePushDevices)
    .where(
      or(
        eq(mobilePushDevices.installationId, installationId),
        and(
          eq(mobilePushDevices.deviceToken, deviceToken),
          eq(mobilePushDevices.bundleId, bundleId),
          eq(mobilePushDevices.apnsEnvironment, request.environment),
        ),
      ),
    )
    .run();

  db.insert(mobilePushDevices)
    .values({
      installationId,
      deviceToken,
      bundleId,
      apnsEnvironment: request.environment,
      deviceName: trimNonEmptyString(request.deviceName) ?? null,
      pushEnabled: true,
      createdAt: timestamp,
      updatedAt: timestamp,
      lastRegisteredAt: timestamp,
      lastDeliveredAt: null,
      lastDeliveryError: null,
    })
    .run();

  const apnsConfig = await loadApnsProviderConfig();

  return {
    state: isProviderReadyForDevice(apnsConfig, request)
      ? MOBILE_PUSH_ENABLED_STATE
      : MOBILE_PUSH_PENDING_STATE,
    environment: request.environment,
    registeredAt: timestamp,
    message: isProviderReadyForDevice(apnsConfig, request)
      ? "Remote push is ready on this Mac."
      : "This iPhone is registered. Add APNs provider credentials on the Mac to deliver remote pushes.",
  };
}

function buildTestPushMessage(device: StoredPushDevice): ApnsAlertMessage {
  return {
    title: APNS_TEST_TITLE,
    subtitle: APNS_TEST_SUBTITLE,
    body: APNS_TEST_BODY,
    threadId: device.installationId,
    userInfo: {
      notificationKind: "test",
    },
  };
}

async function updatePushDeliveryResult(
  installationId: string,
  result: {
    delivered: boolean;
    reason: string | null;
  },
) {
  const timestamp = nowIsoString();
  const shouldDisableDevice = result.reason !== null && APNS_DISABLE_REASONS.has(result.reason);
  const db = getDatabase();

  db.update(mobilePushDevices)
    .set({
      pushEnabled: shouldDisableDevice ? false : true,
      updatedAt: timestamp,
      lastDeliveredAt: result.delivered ? timestamp : null,
      lastDeliveryError: result.delivered ? null : (result.reason ?? "Unknown APNs error"),
    })
    .where(eq(mobilePushDevices.installationId, installationId))
    .run();
}

function getDeviceConfigMismatchMessage(device: StoredPushDevice) {
  return [
    "APNs provider credentials are present, but they do not match this device registration.",
    `Expected topic ${device.bundleId} in the ${device.apnsEnvironment} environment.`,
  ].join(" ");
}

async function loadRegisteredPushDevice(installationId: string) {
  const db = getDatabase();
  return db
    .select()
    .from(mobilePushDevices)
    .where(eq(mobilePushDevices.installationId, installationId))
    .get();
}

export async function sendTestPushToInstallation(
  installationId: string,
): Promise<MobilePushTestResponse> {
  const device = await loadRegisteredPushDevice(installationId);
  if (!device || !device.pushEnabled) {
    return {
      delivered: false,
      message: "This iPhone has not completed remote push registration on the Mac yet.",
    };
  }

  const apnsConfig = await loadApnsProviderConfig();
  if (!apnsConfig) {
    return {
      delivered: false,
      message:
        "The iPhone is registered, but APNs provider credentials are still missing on the Mac.",
    };
  }

  if (
    !isProviderReadyForDevice(apnsConfig, {
      bundleId: device.bundleId,
      environment: device.apnsEnvironment as MobilePushEnvironment,
    })
  ) {
    return {
      delivered: false,
      message: getDeviceConfigMismatchMessage(device),
    };
  }

  const response = await sendApnsAlert(
    apnsConfig,
    device.deviceToken,
    buildTestPushMessage(device),
  );
  await updatePushDeliveryResult(device.installationId, {
    delivered: response.ok,
    reason: response.reason,
  });

  return {
    delivered: response.ok,
    message: response.ok
      ? "Test push sent."
      : `APNs rejected the test push${response.reason ? `: ${response.reason}` : "."}`,
  };
}
