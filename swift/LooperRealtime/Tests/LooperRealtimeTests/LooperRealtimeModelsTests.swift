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
}
