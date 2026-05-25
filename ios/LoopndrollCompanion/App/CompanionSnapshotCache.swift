import Foundation

enum CompanionSnapshotCache {
    private static let snapshotKey = "looper.cachedMobileSnapshot.v1"

    static func load() -> MobileSnapshot? {
        guard let data = UserDefaults.standard.data(forKey: snapshotKey) else {
            return nil
        }

        return try? JSONDecoder().decode(MobileSnapshot.self, from: data)
    }

    static func save(_ snapshot: MobileSnapshot) {
        guard let data = try? JSONEncoder().encode(snapshot) else {
            return
        }

        UserDefaults.standard.set(data, forKey: snapshotKey)
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: snapshotKey)
    }
}
