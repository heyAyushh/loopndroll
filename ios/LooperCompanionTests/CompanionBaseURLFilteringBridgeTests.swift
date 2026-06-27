import Foundation
import Testing
@testable import Looper

@Suite("Companion app base URL filtering")
struct CompanionBaseURLFilteringBridgeTests {
    @Test
    func appFilteringUsesCoreRoutePolicy() throws {
        let lanURL = try #require(URL(string: "http://192.168.1.33:8765"))
        let loopbackURL = try #require(URL(string: "http://127.0.0.1:8765"))

        let urls = CompanionConfiguration.uniqueAttemptableBaseURLs([
            lanURL,
            loopbackURL,
        ])

        #if targetEnvironment(simulator)
            #expect(urls.map(\.absoluteString) == [
                loopbackURL.absoluteString,
                lanURL.absoluteString,
            ])
        #else
            #expect(urls.map(\.absoluteString) == [
                lanURL.absoluteString,
            ])
        #endif
    }
}
