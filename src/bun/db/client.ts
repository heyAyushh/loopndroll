import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { Database } from "bun:sqlite";
import { drizzle } from "drizzle-orm/bun-sqlite";
import { applyAppMigrations } from "./migrations";
import * as schema from "./schema";

export const SQLITE_PRAGMA_STATEMENTS = [
  "pragma journal_mode = wal",
  "pragma synchronous = normal",
  "pragma foreign_keys = on",
  "pragma busy_timeout = 5000",
] as const;

type LoopndrollDatabase = {
  path: string;
  client: Database;
  db: ReturnType<typeof drizzle<typeof schema>>;
};

let cachedDatabase: LoopndrollDatabase | null = null;

function configureDatabase(client: Database) {
  for (const statement of SQLITE_PRAGMA_STATEMENTS) {
    client.exec(statement);
  }
}

export function getLoopndrollDatabase(databasePath: string) {
  if (cachedDatabase && cachedDatabase.path === databasePath) {
    return cachedDatabase;
  }

  mkdirSync(dirname(databasePath), { recursive: true });

  const client = new Database(databasePath, { create: true });
  configureDatabase(client);
  applyAppMigrations(client);

  cachedDatabase = {
    path: databasePath,
    client,
    db: drizzle(client, { schema }),
  };

  return cachedDatabase;
}
