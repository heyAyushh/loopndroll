import Foundation
import LooperCompanionCore

struct LooperCurrentSessionResolver: Sendable {
    func currentEntity(from snapshot: MobileSnapshot) throws -> LooperSessionEntity {
        guard let resolution = LooperCurrentSessionResolution.resolve(
            currentSessionID: snapshot.globalSettings.siriCurrentSessionId,
            currentAssistantSurface: snapshot.globalSettings.siriCurrentAssistantSurface?.rawValue,
            defaultSessionID: snapshot.globalSettings.siriDefaultSessionId,
            defaultAssistantSurface: snapshot.globalSettings.siriDefaultAssistantSurface?.rawValue,
            candidates: resolutionCandidates(from: snapshot),
            fallbackAssistantSurface: CompanionAssistantSurface.defaultSurface.rawValue
        ) else {
            throw unresolvedCurrentSessionError(for: snapshot)
        }

        guard let assistantSurface = CompanionAssistantSurface(rawValue: resolution.assistantSurface),
              let session = snapshot.sessions(for: assistantSurface).first(where: { session in
                  session.id == resolution.sessionID && !session.isArchived
              })
        else {
            throw LooperSiriError.defaultSessionUnavailable(resolution.sessionID)
        }

        return LooperSessionEntity(
            session: session,
            assistantSurface: assistantSurface
        )
    }

    private func resolutionCandidates(from snapshot: MobileSnapshot) -> [LooperSessionResolutionCandidate] {
        CompanionAssistantSurface.allCases.flatMap { assistantSurface in
            snapshot.sessions(for: assistantSurface).map { session in
                LooperSessionResolutionCandidate(
                    sessionID: session.id,
                    assistantSurface: assistantSurface.rawValue,
                    isArchived: session.isArchived
                )
            }
        }
    }

    private func unresolvedCurrentSessionError(for snapshot: MobileSnapshot) -> LooperSiriError {
        guard let defaultSessionID = snapshot.globalSettings.siriDefaultSessionId else {
            return .noDefaultSession
        }

        return .defaultSessionUnavailable(defaultSessionID)
    }
}
