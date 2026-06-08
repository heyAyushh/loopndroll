import Foundation

public struct LooperRealtimeEndpoint: Equatable, Sendable {
    public let baseURL: URL

    public init(baseURL: URL) {
        self.baseURL = baseURL
    }

    public var host: String? {
        baseURL.host()
    }

    public var port: Int? {
        baseURL.port
    }

    public var usesTLS: Bool {
        baseURL.scheme == "https"
    }
}

public struct LooperRealtimeCredentials: Equatable, Sendable {
    public let bearerToken: String?
    public let mobileSessionHeader: String?

    public init(bearerToken: String?, mobileSessionHeader: String?) {
        self.bearerToken = bearerToken
        self.mobileSessionHeader = mobileSessionHeader
    }
}

public struct LooperRealtimeEvent: Equatable, Sendable {
    public let eventName: String
    public let threadID: String?
    public let promptID: String?
    public let detail: String?
    public let serverTime: String?
    public let revision: String?

    public init(
        eventName: String,
        threadID: String?,
        promptID: String?,
        detail: String?,
        serverTime: String?,
        revision: String?
    ) {
        self.eventName = eventName
        self.threadID = threadID
        self.promptID = promptID
        self.detail = detail
        self.serverTime = serverTime
        self.revision = revision
    }
}

public struct LooperRealtimePromptResponse: Equatable, Sendable {
    public let accepted: Bool
    public let dispatchKind: String
    public let promptID: String?

    public init(accepted: Bool, dispatchKind: String, promptID: String?) {
        self.accepted = accepted
        self.dispatchKind = dispatchKind
        self.promptID = promptID
    }
}

public enum LooperRealtimeError: Error, Equatable {
    case invalidEndpoint
    case unavailable
}
