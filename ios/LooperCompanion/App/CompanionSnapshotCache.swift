import Foundation

/// Vestige of the removed legacy disk snapshot cache. The session-mini
/// client-core store is the single local source of truth now; only `clear()`
/// remains so connection resets and UI-test state resets can wipe any cache
/// files left behind by older builds.
enum CompanionSnapshotCache {
    private static let snapshotKey = "looper.cachedMobileSnapshot.v1"
    private static let snapshotFilename = "mobile-snapshot-cache.json"

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
