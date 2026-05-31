import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { describe, expect, test } from "bun:test";
import {
  issueMobilePairingToken,
  mobileAuthorizationHeader,
  validateMobileAuthorizationHeader,
} from "./mobile-auth";

function createTestDatabasePath() {
  const directoryPath = join(
    import.meta.dir,
    "..",
    "..",
    "target",
    "mobile-auth-tests",
    randomUUID(),
  );
  mkdirSync(directoryPath, { recursive: true });
  return join(directoryPath, "app.db");
}

describe("mobile pairing auth", () => {
  test("accepts only issued bearer tokens", () => {
    const databasePath = createTestDatabasePath();
    const token = issueMobilePairingToken(databasePath);
    const authorizationHeader = mobileAuthorizationHeader(token);

    expect(validateMobileAuthorizationHeader(authorizationHeader, databasePath)).toBe(true);
    expect(validateMobileAuthorizationHeader(`${authorizationHeader}-wrong`, databasePath)).toBe(
      false,
    );
    expect(validateMobileAuthorizationHeader(null, databasePath)).toBe(false);
  });
});
