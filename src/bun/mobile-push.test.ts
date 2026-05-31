import { describe, expect, test } from "bun:test";
import { generateKeyPairSync } from "node:crypto";
import { createApnsJwt, loadApnsProviderConfigFromSources } from "./mobile-push";

function decodeBase64UrlJson<T>(value: string) {
  const normalizedValue = value.replaceAll("-", "+").replaceAll("_", "/");
  const paddedValue = normalizedValue.padEnd(Math.ceil(normalizedValue.length / 4) * 4, "=");
  return JSON.parse(Buffer.from(paddedValue, "base64").toString("utf8")) as T;
}

describe("loadApnsProviderConfigFromSources", () => {
  test("prefers environment values over the config file", async () => {
    const result = await loadApnsProviderConfigFromSources(
      {
        LOOPER_APNS_KEY_ID: "env-key",
        LOOPER_APNS_TEAM_ID: "env-team",
        LOOPER_APNS_BUNDLE_ID: "dev.looper.app.ios",
        LOOPER_APNS_ENVIRONMENT: "production",
        LOOPER_APNS_PRIVATE_KEY_PEM:
          "-----BEGIN PRIVATE KEY-----\nZXhhbXBsZQ==\n-----END PRIVATE KEY-----",
      },
      {
        keyId: "file-key",
        teamId: "file-team",
        bundleId: "file.bundle",
        environment: "development",
        privateKeyPem: "file-pem",
      },
    );

    expect(result).toEqual({
      keyId: "env-key",
      teamId: "env-team",
      bundleId: "dev.looper.app.ios",
      environment: "production",
      privateKeyPem: "-----BEGIN PRIVATE KEY-----\nZXhhbXBsZQ==\n-----END PRIVATE KEY-----",
    });
  });

  test("returns null when required values are missing", async () => {
    const result = await loadApnsProviderConfigFromSources({}, {});
    expect(result).toBeNull();
  });
});

describe("createApnsJwt", () => {
  test("creates a token with the expected header and claims", async () => {
    const { privateKey } = generateKeyPairSync("ec", {
      namedCurve: "prime256v1",
    });
    const privateKeyPem = privateKey
      .export({
        type: "pkcs8",
        format: "pem",
      })
      .toString();
    const issuedAt = 1_762_000_000;
    const token = await createApnsJwt(
      {
        keyId: "ABC123XYZ",
        teamId: "TEAM123456",
        bundleId: "dev.looper.app.ios",
        environment: "production",
        privateKeyPem,
      },
      issuedAt,
    );

    const [headerSegment, claimsSegment, signatureSegment] = token.split(".");
    expect(headerSegment).toBeTruthy();
    expect(claimsSegment).toBeTruthy();
    expect(signatureSegment).toBeTruthy();

    expect(decodeBase64UrlJson<{ alg: string; kid: string }>(headerSegment ?? "")).toEqual({
      alg: "ES256",
      kid: "ABC123XYZ",
    });
    expect(decodeBase64UrlJson<{ iss: string; iat: number }>(claimsSegment ?? "")).toEqual({
      iss: "TEAM123456",
      iat: issuedAt,
    });
    expect(signatureSegment?.length).toBeGreaterThan(80);
  });
});
