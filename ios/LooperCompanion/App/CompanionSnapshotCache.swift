import Foundation

enum CompanionSnapshotCache {
    private static let snapshotKey = "looper.cachedMobileSnapshot.v1"
    private static let snapshotFilename = "mobile-snapshot-cache.json"

    static func load() async -> MobileSnapshot? {
        await Task.detached(priority: .userInitiated) {
            loadFromDisk()
        }.value
    }

    private static func loadFromDisk() -> MobileSnapshot? {
        let url = snapshotURL()

        if FileManager.default.fileExists(atPath: url.path) {
            let data: Data
            do {
                data = try Data(contentsOf: url)
            } catch {
                CompanionDiagnostics.cache.error(
                    "Failed to read snapshot cache error=\(error.localizedDescription, privacy: .public)"
                )
                CompanionDiagnostics.record("cache:read-failed error=\(error.localizedDescription)")
                return nil
            }

            do {
                let snapshot = try JSONDecoder().decode(MobileSnapshot.self, from: data)
                CompanionDiagnostics.cache.info("Loaded file snapshot cache hasSnapshot=true")
                CompanionDiagnostics.record("cache:load-file hasSnapshot=true")
                return snapshot
            } catch {
                CompanionDiagnostics.cache.error(
                    "Failed to decode snapshot cache error=\(error.localizedDescription, privacy: .public)"
                )
                CompanionDiagnostics.cache.info(
                    "Loaded file snapshot cache hasSnapshot=false"
                )
                CompanionDiagnostics.record(
                    "cache:decode-failed error=\(error.localizedDescription)"
                )
                return nil
            }
        }

        if let legacyData = UserDefaults.standard.data(forKey: snapshotKey) {
            do {
                let snapshot = try JSONDecoder().decode(MobileSnapshot.self, from: legacyData)
                save(snapshot)
                CompanionDiagnostics.cache.info("Migrated legacy snapshot cache")
                CompanionDiagnostics.record("cache:migrate-legacy")
                return snapshot
            } catch {
                CompanionDiagnostics.cache.error(
                    "Failed to decode legacy snapshot cache error=\(error.localizedDescription, privacy: .public)"
                )
                CompanionDiagnostics.record(
                    "cache:legacy-decode-failed error=\(error.localizedDescription)"
                )
            }
        }

        CompanionDiagnostics.cache.info("No snapshot cache found")
        CompanionDiagnostics.record("cache:miss")
        return nil
    }

    static func save(_ snapshot: MobileSnapshot) {
        Task.detached(priority: .utility) {
            let data: Data
            do {
                data = try JSONEncoder().encode(snapshot)
            } catch {
                CompanionDiagnostics.cache.error(
                    "Failed to encode snapshot cache error=\(error.localizedDescription, privacy: .public)"
                )
                return
            }

            do {
                try data.write(to: snapshotURL(), options: .atomic)
                CompanionDiagnostics.cache.info(
                    "Saved snapshot cache sessions=\(snapshot.sessions.count, privacy: .public)"
                )
                CompanionDiagnostics.record("cache:save sessions=\(snapshot.sessions.count)")
            } catch {
                CompanionDiagnostics.cache.error(
                    "Failed to write snapshot cache error=\(error.localizedDescription, privacy: .public)"
                )
                CompanionDiagnostics.record("cache:write-failed error=\(error.localizedDescription)")
            }
            UserDefaults.standard.removeObject(forKey: snapshotKey)
        }
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: snapshotKey)
        try? FileManager.default.removeItem(at: snapshotURL())
        CompanionDiagnostics.cache.info("Cleared snapshot cache")
        CompanionDiagnostics.record("cache:clear")
    }

    private static func snapshotURL() -> URL {
        let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        return cacheDirectory.appendingPathComponent(snapshotFilename)
    }
}
