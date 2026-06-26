import Foundation
import LooperClientCore

struct CompanionPromptSendResult: Sendable {
    let promptID: String?
    let dispatchKind: String?

    static func accepted(
        promptID: String?,
        dispatchKind: String?
    ) -> Self {
        Self(
            promptID: promptID,
            dispatchKind: dispatchKind
        )
    }
}

struct CompanionSessionModeResult: Sendable {
    let acceptedMode: SessionMode?

    static func accepted(
        mode: SessionMode?
    ) -> Self {
        Self(
            acceptedMode: mode
        )
    }
}
