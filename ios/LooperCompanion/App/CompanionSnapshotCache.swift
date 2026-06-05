import Foundation

enum CompanionSnapshotCache {
    private static let snapshotKey = "looper.cachedMobileSnapshot.v1"
    private static let snapshotFilename = "mobile-snapshot-cache.json"

    static func load() -> MobileSnapshot? {
        if let data = try? Data(contentsOf: snapshotURL()) {
            let snapshot = try? JSONDecoder().decode(MobileSnapshot.self, from: data)
            CompanionDiagnostics.cache.info(
                "Loaded file snapshot cache hasSnapshot=\(snapshot != nil, privacy: .public)"
            )
            CompanionDiagnostics.record("cache:load-file hasSnapshot=\(snapshot != nil)")
            return snapshot
        }

        if let legacyData = UserDefaults.standard.data(forKey: snapshotKey),
           let snapshot = try? JSONDecoder().decode(MobileSnapshot.self, from: legacyData) {
            save(snapshot)
            CompanionDiagnostics.cache.info("Migrated legacy snapshot cache")
            CompanionDiagnostics.record("cache:migrate-legacy")
            return snapshot
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
