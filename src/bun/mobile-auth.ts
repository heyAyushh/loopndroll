import { createHash, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import { getLoopndrollDatabase } from "./db/client";
import { getLoopndrollPaths } from "./loopndroll-core";

const BEARER_PREFIX = "Bearer ";
const MOBILE_PAIRING_TOKEN_BYTES = 32;

type PairingTokenRow = {
  token_hash: string;
};

export type IssuedMobilePairingToken = {
  id: string;
  token: string;
};

function nowIsoString() {
  return new Date().toISOString();
}

function encodeBase64URL(value: Buffer) {
  return value.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

function hashPairingToken(token: string) {
  return encodeBase64URL(createHash("sha256").update(token, "utf8").digest());
}

function parseBearerToken(authorizationHeader: string | null) {
  if (!authorizationHeader?.startsWith(BEARER_PREFIX)) {
    return null;
  }

  const credential = authorizationHeader.slice(BEARER_PREFIX.length).trim();
  const separatorIndex = credential.indexOf(".");
  if (separatorIndex <= 0 || separatorIndex === credential.length - 1) {
    return null;
  }

  return {
    id: credential.slice(0, separatorIndex),
    token: credential.slice(separatorIndex + 1),
  };
}

function constantTimeEquals(left: string, right: string) {
  const leftBuffer = Buffer.from(left);
  const rightBuffer = Buffer.from(right);
  return leftBuffer.length === rightBuffer.length && timingSafeEqual(leftBuffer, rightBuffer);
}

export function issueMobilePairingToken(databasePath = getLoopndrollPaths().databasePath) {
  const { client } = getLoopndrollDatabase(databasePath);
  const issuedToken: IssuedMobilePairingToken = {
    id: randomUUID(),
    token: encodeBase64URL(randomBytes(MOBILE_PAIRING_TOKEN_BYTES)),
  };

  client
    .query(
      `insert into mobile_pairing_tokens (
        id,
        token_hash,
        label,
        created_at,
        last_used_at,
        revoked_at
      ) values (?, ?, ?, ?, null, null)`,
    )
    .run(issuedToken.id, hashPairingToken(issuedToken.token), "iPhone companion", nowIsoString());

  return issuedToken;
}

export function validateMobileAuthorizationHeader(
  authorizationHeader: string | null,
  databasePath = getLoopndrollPaths().databasePath,
) {
  const credential = parseBearerToken(authorizationHeader);
  if (!credential) {
    return false;
  }

  const { client } = getLoopndrollDatabase(databasePath);
  const row = client
    .query(
      `select token_hash
      from mobile_pairing_tokens
      where id = ?
        and revoked_at is null
      limit 1`,
    )
    .get(credential.id) as PairingTokenRow | null;

  if (!row) {
    return false;
  }

  const isValid = constantTimeEquals(row.token_hash, hashPairingToken(credential.token));
  if (isValid) {
    client
      .query("update mobile_pairing_tokens set last_used_at = ? where id = ?")
      .run(nowIsoString(), credential.id);
  }

  return isValid;
}

export function mobileAuthorizationHeader(token: IssuedMobilePairingToken) {
  return `${BEARER_PREFIX}${token.id}.${token.token}`;
}
