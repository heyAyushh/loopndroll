import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Companion connection route presentation")
struct CompanionConnectionRoutePresentationTests {
    private let tailscaleURL = "http://100.95.2.4:8765"
    private let magicDNSURL = "http://ayushs-macbook-pro.tail62d9a8.ts.net:8765"
    private let lanURL = "http://192.168.2.10:8765"
    private let remoteURL = "https://looper.example.test"
    private let loopbackURL = "http://127.0.0.1:8765"
    private let unsupportedURL = "preview://looper"

    @Test("Tailscale routes use the Tailscale presentation")
    func tailscaleRoutesUseTailscalePresentation() throws {
        let presentation = try #require(CompanionConnectionRoutePresentation(
            baseURL: url(tailscaleURL),
            tailscaleDetail: "ayushs-macbook-pro.tail62d9a8.ts.net"
        ))

        #expect(presentation.route == .tailscale)
        #expect(presentation.title == "Tailscale")
        #expect(presentation.detail == "ayushs-macbook-pro.tail62d9a8.ts.net")
        #expect(presentation.usesTailscaleLogo)
    }

    @Test("MagicDNS routes are presented as Tailscale")
    func magicDNSRoutesArePresentedAsTailscale() throws {
        let presentation = try #require(CompanionConnectionRoutePresentation(
            baseURL: url(magicDNSURL)
        ))

        #expect(presentation.route == .tailscale)
        #expect(presentation.title == "Tailscale")
        #expect(presentation.detail == "ayushs-macbook-pro.tail62d9a8.ts.net")
        #expect(presentation.usesTailscaleLogo)
    }

    @Test("LAN and remote routes keep their own labels")
    func lanAndRemoteRoutesKeepTheirOwnLabels() throws {
        let lanPresentation = try #require(CompanionConnectionRoutePresentation(baseURL: url(lanURL)))
        let remotePresentation = try #require(CompanionConnectionRoutePresentation(baseURL: url(remoteURL)))

        #expect(lanPresentation.title == "LAN")
        #expect(!lanPresentation.usesTailscaleLogo)
        #expect(remotePresentation.title == "Remote")
        #expect(!remotePresentation.usesTailscaleLogo)
    }

    @Test("Unsupported routes do not render a home badge")
    func unsupportedRoutesDoNotRenderHomeBadge() throws {
        let unsupportedBaseURL = try url(unsupportedURL)

        #expect(CompanionConnectionRoutePresentation(baseURL: unsupportedBaseURL) == nil)
    }

    @Test("Reached URL wins over configured URL for the displayed route")
    func reachedURLWinsOverConfiguredURLForDisplayedRoute() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: url(lanURL),
                configuredBaseURL: url(remoteURL),
                healthBaseURL: url(remoteURL),
                tailscaleHealthBaseURL: nil,
                isTailscaleRunning: false
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == lanURL)
        #expect(presentation.route == .lan)
        #expect(presentation.title == "LAN")
    }

    @Test("Reached Tailscale URL stays visible even when health has not caught up")
    func reachedTailscaleURLStaysVisibleEvenWhenHealthHasNotCaughtUp() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: url(tailscaleURL),
                configuredBaseURL: url(lanURL),
                healthBaseURL: url(lanURL),
                tailscaleHealthBaseURL: nil,
                isTailscaleRunning: false
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == tailscaleURL)
        #expect(presentation.route == .tailscale)
        #expect(presentation.title == "Tailscale")
    }

    @Test("Stopped configured Tailscale URL falls back to the reached LAN route")
    func stoppedConfiguredTailscaleURLFallsBackToReachedLANRoute() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: url(lanURL),
                configuredBaseURL: url(tailscaleURL),
                healthBaseURL: url(remoteURL),
                tailscaleHealthBaseURL: url(tailscaleURL),
                isTailscaleRunning: false
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == lanURL)
        #expect(presentation.route == .lan)
    }

    @Test("Stopped configured Tailscale URL falls back to health when nothing was reached")
    func stoppedConfiguredTailscaleURLFallsBackToHealthWhenNothingWasReached() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: nil,
                configuredBaseURL: url(tailscaleURL),
                healthBaseURL: url(remoteURL),
                tailscaleHealthBaseURL: url(tailscaleURL),
                isTailscaleRunning: false
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == remoteURL)
        #expect(presentation.route == .remote)
    }

    @Test("Active Tailscale health is not hidden by a loopback health primary")
    func activeTailscaleHealthIsNotHiddenByLoopbackHealthPrimary() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: nil,
                configuredBaseURL: nil,
                healthBaseURL: url(loopbackURL),
                tailscaleHealthBaseURL: url(tailscaleURL),
                isTailscaleRunning: true
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == tailscaleURL)
        #expect(presentation.route == .tailscale)
    }

    @Test("Configured remote route is used when no URL has been reached")
    func configuredRemoteRouteIsUsedWhenNoURLHasBeenReached() throws {
        let selectedBaseURL = try #require(
            CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
                reachedBaseURL: nil,
                configuredBaseURL: url(remoteURL),
                healthBaseURL: url(lanURL),
                tailscaleHealthBaseURL: url(tailscaleURL),
                isTailscaleRunning: true
            )
        )
        let presentation = try #require(CompanionConnectionRoutePresentation(baseURL: selectedBaseURL))

        #expect(selectedBaseURL.absoluteString == remoteURL)
        #expect(presentation.route == .remote)
    }

    private func url(_ value: String) throws -> URL {
        try #require(URL(string: value))
    }
}
