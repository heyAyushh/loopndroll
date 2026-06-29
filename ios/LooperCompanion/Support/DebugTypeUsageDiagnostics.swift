#if DEBUG
import Foundation
import Reaper

enum DebugTypeUsageDiagnostics {
    private static let looperTypePrefix = "Looper."
    private static let interestingTypeFragments = [
        "AssistantSurfacePicker",
        "CompanionAppModel",
        "CompanionSnapshotStateStore",
        "SessionConnectionRow",
        "SessionDetailScreen",
        "SessionRow",
        "SessionsScreen",
    ]

    static func startIfNeeded(isRunningUnitTests: Bool, isRunningSelfTest: Bool) {
        guard !isRunningUnitTests, !isRunningSelfTest else {
            return
        }

        CompanionDiagnostics.record("reaper:start-requested")
        EMGReaper.sharedInstance().start { usedTypes in
            record(usedTypes: usedTypes)
        }
    }

    private static func record(usedTypes: [String]) {
        let looperTypes = usedTypes
            .filter { typeName in
                typeName.hasPrefix(looperTypePrefix) ||
                    interestingTypeFragments.contains { fragment in
                        typeName.contains(fragment)
                    }
            }
            .sorted()

        CompanionDiagnostics.record(
            "reaper:used-types total=\(usedTypes.count) looper=\(looperTypes.count)"
        )
        guard !looperTypes.isEmpty else {
            return
        }

        CompanionDiagnostics.record(
            "reaper:looper-types \(looperTypes.joined(separator: ","))"
        )
    }
}
#endif
