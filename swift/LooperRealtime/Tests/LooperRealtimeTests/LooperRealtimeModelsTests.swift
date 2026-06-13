import Foundation
import Testing

@testable import LooperRealtime

struct LooperRealtimeModelsTests {
    @Test
    func endpointExposesHostPortAndTLS() throws {
        let endpoint = try #require(URL(string: "https://192.168.1.4:8766"))
        let realtimeEndpoint = LooperRealtimeEndpoint(baseURL: endpoint)

        #expect(realtimeEndpoint.host == "192.168.1.4")
        #expect(realtimeEndpoint.port == 8766)
        #expect(realtimeEndpoint.usesTLS)
    }

    @Test
    func endpointIsStablePoolKey() throws {
        let firstURL = try #require(URL(string: "http://127.0.0.1:8766"))
        let secondURL = try #require(URL(string: "http://127.0.0.1:8766"))
        let firstEndpoint = LooperRealtimeEndpoint(baseURL: firstURL)
        let secondEndpoint = LooperRealtimeEndpoint(baseURL: secondURL)

        #expect(Set([firstEndpoint, secondEndpoint]).count == 1)
    }
}
