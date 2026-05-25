import OrbCodeKit
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

private enum OrbScannerMetrics {
    static let sheetOpenFraction = 0.42
}

private enum OrbScanSource: Sendable {
    case camera
    case photos
    case files

    var label: String {
        switch self {
        case .camera:
            return "Live Camera"
        case .photos:
            return "Photos"
        case .files:
            return "Files"
        }
    }
}

private enum OrbVerificationState: Sendable {
    case imageVerified
    case imageRecovered(String)
    case livePayloadAccepted
    case failed(String)

    var label: String {
        switch self {
        case .imageVerified:
            return "Image Verified"
        case .imageRecovered:
            return "Image Decoded"
        case .livePayloadAccepted:
            return "Live Payload"
        case .failed:
            return "Failed"
        }
    }

    var detail: String? {
        switch self {
        case .imageVerified:
            return nil
        case let .imageRecovered(message):
            return message
        case .livePayloadAccepted:
            return "The live scanner decoded the orb from a camera frame and stopped after the first hit."
        case let .failed(message):
            return message
        }
    }
}

private enum OrbScannerFeedbackState {
    case searching
    case checking
    case detected
    case failed

    var label: String {
        switch self {
        case .searching:
            return "No Orb Yet"
        case .checking:
            return "Checking"
        case .detected:
            return "Orb Locked"
        case .failed:
            return "Failed"
        }
    }

    var systemImage: String {
        switch self {
        case .searching:
            return "dot.radiowaves.left.and.right"
        case .checking:
            return "hourglass"
        case .detected:
            return "checkmark.circle.fill"
        case .failed:
            return "exclamationmark.triangle.fill"
        }
    }

    var tint: Color {
        switch self {
        case .searching:
            return .secondary
        case .checking:
            return .blue
        case .detected:
            return .green
        case .failed:
            return .red
        }
    }
}

private struct ImportedOrbScan: Sendable {
    let orbID: String
    let verificationState: OrbVerificationState
}

private enum OrbScannerImportError: Error, LocalizedError, Sendable {
    case unsupportedImage
    case notRecognized

    var errorDescription: String? {
        switch self {
        case .unsupportedImage:
            return "That file is not a readable image. Choose the generated orb PNG or a screenshot."
        case .notRecognized:
            return "No orb was found in that image. Upload the generated orb PNG directly, or use a sharper screenshot where the orb fills most of the image."
        }
    }
}

private struct OrbScanReport: Sendable {
    let orbID: String
    let source: OrbScanSource
    let scannedAt: Date
    let verificationState: OrbVerificationState
    let expectedOrbID: String?

    var expectationLabel: String {
        guard let expectedOrbID else {
            return "Not Compared"
        }

        return expectedOrbID == orbID ? "Matched" : "Did Not Match"
    }

    var expectationMatches: Bool? {
        guard let expectedOrbID else {
            return nil
        }

        return expectedOrbID == orbID
    }
}

struct OrbScannerScreen: View {
    @Environment(\.dismiss) private var dismiss

    @State private var cameraAvailability: LiveOrbCameraAvailability = .ready
    @State private var expectedOrbID = ""
    @State private var isControlsSheetPresented = true
    @State private var isFileImporterPresented = false
    @State private var isLiveScanningEnabled = true
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var liveCameraDiagnostics = LiveOrbCameraDiagnostics()
    @State private var cameraResetToken = LiveOrbCameraResetDefaults.initialToken
    @State private var scanReport: OrbScanReport?
    @State private var errorMessage: String?
    @State private var isProcessing = false
    @FocusState private var isExpectedOrbIDFocused: Bool

    private let dateFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter
    }()

    var body: some View {
        NavigationStack {
            ZStack {
                Color.black.ignoresSafeArea()
                cameraPreview

                if case let .unavailable(message) = cameraAvailability {
                    ContentUnavailableView(
                        "Camera Unavailable",
                        systemImage: "camera.fill.badge.xmark",
                        description: Text(message)
                    )
                    .foregroundStyle(.white)
                    .padding(24)
                }
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                    }
                    .accessibilityLabel("Close scanner")
                }

                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        openControlsSheet()
                    } label: {
                        Image(systemName: "slider.horizontal.3")
                    }
                    .accessibilityLabel("Scanner controls")
                }
            }
            .toolbarBackground(.hidden, for: .navigationBar)
            .toolbarColorScheme(.dark, for: .navigationBar)
        }
        .sheet(isPresented: $isControlsSheetPresented) {
            scannerControlsSheet
                .presentationDetents([
                    .fraction(OrbScannerMetrics.sheetOpenFraction),
                    .large,
                ])
                .presentationDragIndicator(.visible)
                .presentationBackground(.regularMaterial)
                .presentationBackgroundInteraction(
                    .enabled(upThrough: .fraction(OrbScannerMetrics.sheetOpenFraction))
                )
        }
        .fileImporter(
            isPresented: $isFileImporterPresented,
            allowedContentTypes: [.png, .image],
            allowsMultipleSelection: false
        ) { result in
            handleImportedFile(result)
        }
        .task(id: selectedPhotoItem) {
            guard let selectedPhotoItem else {
                return
            }

            await handleSelectedPhotoItem(selectedPhotoItem)
        }
    }

    private var cameraPreview: some View {
        ZStack {
            LiveOrbCameraScannerView(
                isScanningEnabled: isLiveScanningEnabled && !isProcessing,
                payloadTransformer: nil,
                imageDataTransformer: nil,
                lumaTransformer: { data, width, height in
                    // The camera delivers 8-bit luma in plane 0 of the pixel buffer, so
                    // the decoder skips PNG decode + RGB→gray and works on the raw bytes.
                    try OrbCodeKit.scanOrbID(fromLuma8: data, width: width, height: height)
                },
                onValueDetected: handleLiveOrbID,
                onAvailabilityChange: handleCameraAvailabilityChange,
                onDiagnosticsChange: handleLiveDiagnosticsChange,
                resetToken: cameraResetToken
            )
            .ignoresSafeArea()

            if isProcessing {
                ProgressView("Checking Orb")
                    .font(.footnote.weight(.medium))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(.regularMaterial, in: Capsule())
            }
        }
        .ignoresSafeArea()
    }

    private var scannerStatusMessage: String {
        if isProcessing {
            return "Checking the first match."
        }

        if isLiveScanningEnabled {
            return "Aim at the orb on your Mac."
        }

        if let scanReport {
            return "Decoded \(scanReport.orbID)."
        }

        return "Aim at the orb."
    }

    private var scannerGuidanceMessage: String? {
        if isProcessing {
            return "Hold steady until it confirms."
        }

        guard isLiveScanningEnabled else {
            return nil
        }

        return "Center the orb in the frame, or paste the code below."
    }

    private var feedbackState: OrbScannerFeedbackState {
        if errorMessage != nil {
            return .failed
        }

        if isProcessing {
            return .checking
        }

        if scanReport != nil || !isLiveScanningEnabled {
            return .detected
        }

        return .searching
    }

    private var cameraStatusLabel: String {
        switch cameraAvailability {
        case .ready:
            return isLiveScanningEnabled ? "Camera Live" : "Camera Ready"
        case .unavailable:
            return "Camera Blocked"
        }
    }

    private var cameraStatusSystemImage: String {
        switch cameraAvailability {
        case .ready:
            return "camera.viewfinder"
        case .unavailable:
            return "camera.fill.badge.xmark"
        }
    }

    private var cameraStatusTint: Color {
        switch cameraAvailability {
        case .ready:
            return .green
        case .unavailable:
            return .red
        }
    }

    private var scannerControlsSheet: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    scannerOutcomeBanner
                    uploadActionSection
                    directCodeSection
                    Divider()
                    scannerSummaryContent
                    scannerActionRow
                }
                .padding(.horizontal, 20)
                .padding(.top, 12)
                .padding(.bottom, 24)
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle("Scan Orb")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        isControlsSheetPresented = false
                    }
                }

                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()

                    Button("Done") {
                        isExpectedOrbIDFocused = false
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var scannerOutcomeBanner: some View {
        if isProcessing {
            scannerBanner(
                title: "Checking Orb",
                message: "Reading the image and trying the camera-frame recovery path.",
                systemImage: "hourglass",
                tint: .blue
            )
        } else if let errorMessage {
            scannerBanner(
                title: "Scan Failed",
                message: errorMessage,
                systemImage: "exclamationmark.triangle.fill",
                tint: .red
            )
        } else if let scanReport {
            scannerBanner(
                title: scanReportBannerTitle(scanReport),
                message: scanReportBannerMessage(scanReport),
                systemImage: scanReportNeedsAttention(scanReport)
                    ? "exclamationmark.triangle.fill"
                    : "checkmark.circle.fill",
                tint: scanReportNeedsAttention(scanReport) ? .red : .green
            )
        }
    }

    private func scannerBanner(
        title: String,
        message: String,
        systemImage: String,
        tint: Color
    ) -> some View {
        Label {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)
                Text(message)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        } icon: {
            Image(systemName: systemImage)
                .foregroundStyle(tint)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func scanReportBannerTitle(_ scanReport: OrbScanReport) -> String {
        if scanReport.expectationMatches == false {
            return "Orb Did Not Match"
        }

        if case .failed = scanReport.verificationState {
            return "Image Decoded, Check Failed"
        }

        return "Orb Decoded"
    }

    private func scanReportBannerMessage(_ scanReport: OrbScanReport) -> String {
        guard let detail = scanReport.verificationState.detail else {
            return "orb_id \(scanReport.orbID)"
        }

        return "orb_id \(scanReport.orbID)\n\(detail)"
    }

    private func scanReportNeedsAttention(_ scanReport: OrbScanReport) -> Bool {
        Self.scanReportNeedsAttention(scanReport)
    }

    private var uploadActionSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Upload")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)

            PhotosPicker(
                selection: $selectedPhotoItem,
                matching: .images,
                photoLibrary: .shared()
            ) {
                Label("Upload Image", systemImage: "photo.on.rectangle")
                    .frame(maxWidth: .infinity, alignment: .center)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)

            Button {
                isFileImporterPresented = true
            } label: {
                Label("Choose from Files", systemImage: "folder")
                    .frame(maxWidth: .infinity, alignment: .center)
            }
            .buttonStyle(.bordered)
            .controlSize(.large)

            Text("Upload uses the exact image. Live scan also has to handle glare, focus, angle, and screen refresh.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var directCodeSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Add Code Directly")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)

            TextField("Paste orb_id directly", text: $expectedOrbID)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.done)
                .font(.body.monospaced())
                .focused($isExpectedOrbIDFocused)
                .onSubmit {
                    isExpectedOrbIDFocused = false
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(
                    .thinMaterial,
                    in: RoundedRectangle(cornerRadius: 16, style: .continuous)
                )

            Text("Paste the orb_id from the CLI to compare the live scan the moment it locks.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var scannerActionRow: some View {
        HStack(spacing: 10) {
            if scanReport != nil || errorMessage != nil || !isLiveScanningEnabled {
                Button("Resume Live Scan") {
                    resumeLiveScanning()
                }
                .buttonStyle(.borderedProminent)
            }

            Button("Reset") {
                resetScanner()
            }
            .buttonStyle(.bordered)
        }
    }

    @ViewBuilder
    private var scannerSummaryContent: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                scannerFeedbackRow
                cameraStatusRow
            }

            if let scanReport {
                resultContent(scanReport)
            } else if let errorMessage {
                errorContent(errorMessage)
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Scan Orb")
                        .font(.headline)
                    Text(scannerStatusMessage)
                        .font(.footnote)
                        .foregroundStyle(.secondary)

                    if let scannerGuidanceMessage {
                        Text(scannerGuidanceMessage)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }

                    liveCameraDiagnosticsContent
                }
            }
        }
    }

    @ViewBuilder
    private var liveCameraDiagnosticsContent: some View {
        if isLiveScanningEnabled {
            VStack(alignment: .leading, spacing: 8) {
                resultRow("Frames checked", value: "\(liveCameraDiagnostics.analyzedFrameCount)")
                resultRow(
                    "Latest check",
                    value: liveCameraLatestCheckLabel,
                    tint: .secondary
                )
            }
            .padding(.top, 4)
        }
    }

    private var liveCameraLatestCheckLabel: String {
        if let orbID = liveCameraDiagnostics.lastDetectedOrbID, !orbID.isEmpty {
            return "Locked \(orbID)"
        }

        guard liveCameraDiagnostics.hasAnalyzedFrames else {
            return "Waiting for the first camera frame"
        }

        guard let lastFailureMessage = liveCameraDiagnostics.lastFailureMessage,
              !lastFailureMessage.isEmpty
        else {
            return "Still scanning"
        }

        return lastFailureMessage
    }

    private var scannerFeedbackRow: some View {
        Label {
            Text(feedbackState.label)
                .font(.footnote.weight(.semibold))
        } icon: {
            Image(systemName: feedbackState.systemImage)
        }
        .foregroundStyle(feedbackState.tint)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.thinMaterial, in: Capsule())
    }

    private var cameraStatusRow: some View {
        Label {
            Text(cameraStatusLabel)
                .font(.footnote.weight(.semibold))
        } icon: {
            Image(systemName: cameraStatusSystemImage)
        }
        .foregroundStyle(cameraStatusTint)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.thinMaterial, in: Capsule())
    }

    private func resultContent(_ scanReport: OrbScanReport) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Result")
                .font(.headline)

            resultRow("Source", value: scanReport.source.label)
            resultRow("Decoded orb_id", value: scanReport.orbID, monospaced: true)
            resultRow(
                "Verification",
                value: scanReport.verificationState.label,
                tint: verificationTint(scanReport.verificationState)
            )

            if let verificationDetail = scanReport.verificationState.detail {
                Text(verificationDetail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            if let expectedOrbID = scanReport.expectedOrbID {
                resultRow("Expected", value: expectedOrbID, monospaced: true)
                resultRow(
                    "Comparison",
                    value: scanReport.expectationLabel,
                    tint: expectationTint(scanReport.expectationMatches)
                )
            }

            resultRow(
                "Checked",
                value: dateFormatter.localizedString(for: scanReport.scannedAt, relativeTo: Date()),
                tint: .secondary
            )
        }
    }

    private func errorContent(_ errorMessage: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Scan Failed")
                .font(.headline)

            Text(errorMessage)
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private func resultRow(
        _ title: String,
        value: String,
        monospaced: Bool = false,
        tint: Color = .primary
    ) -> some View {
        LabeledContent(title) {
            Text(value)
                .font(monospaced ? .footnote.monospaced() : .body)
                .foregroundStyle(tint)
                .multilineTextAlignment(.trailing)
        }
    }

    private func handleCameraAvailabilityChange(_ availability: LiveOrbCameraAvailability) {
        cameraAvailability = availability
    }

    private func handleLiveDiagnosticsChange(_ diagnostics: LiveOrbCameraDiagnostics) {
        liveCameraDiagnostics = diagnostics
    }

    private func handleLiveOrbID(_ orbID: String) {
        guard isLiveScanningEnabled, !isProcessing else {
            return
        }

        isProcessing = true
        let report = makeLiveScanReport(orbID: orbID)

        scanReport = report
        errorMessage = nil
        isLiveScanningEnabled = false
        isProcessing = false
        isControlsSheetPresented = true

        if report.expectationMatches != false {
            Haptics.success()
        }
    }

    private func handleImportedFile(_ result: Result<[URL], Error>) {
        switch result {
        case let .success(urls):
            guard let url = urls.first else {
                return
            }

            beginImportedImageProcessing()

            Task.detached(priority: .userInitiated) {
                do {
                    let data = try Self.readSecurityScopedFileData(from: url)

                    await MainActor.run {
                        processImportedImageData(
                            data,
                            source: .files,
                            missingDataMessage: "The selected file was empty."
                        )
                    }
                } catch {
                    await MainActor.run {
                        errorMessage = error.localizedDescription
                        isProcessing = false
                        isControlsSheetPresented = true
                    }
                }
            }
        case let .failure(error):
            errorMessage = error.localizedDescription
            isControlsSheetPresented = true
        }
    }

    private func handleSelectedPhotoItem(_ item: PhotosPickerItem) async {
        do {
            guard let data = try await item.loadTransferable(type: Data.self) else {
                await MainActor.run {
                    errorMessage = "The selected image could not be loaded."
                    isControlsSheetPresented = true
                    selectedPhotoItem = nil
                }
                return
            }

            await MainActor.run {
                processImportedImageData(
                    data,
                    source: .photos,
                    missingDataMessage: "The selected image was empty."
                )
                selectedPhotoItem = nil
            }
        } catch {
            await MainActor.run {
                errorMessage = error.localizedDescription
                isControlsSheetPresented = true
                selectedPhotoItem = nil
            }
        }
    }

    private func processImportedImageData(
        _ imageData: Data?,
        source: OrbScanSource,
        missingDataMessage: String
    ) {
        guard let imageData, !imageData.isEmpty else {
            errorMessage = missingDataMessage
            isProcessing = false
            isControlsSheetPresented = true
            return
        }

        beginImportedImageProcessing()
        let expectedOrbIDSnapshot = expectedOrbID

        Task.detached(priority: .userInitiated) {
            do {
                let report = try Self.makeImportedScanReport(
                    from: imageData,
                    source: source,
                    expectedOrbID: expectedOrbIDSnapshot
                )

                await MainActor.run {
                    scanReport = report
                    isProcessing = false
                    isControlsSheetPresented = true

                    if !Self.scanReportNeedsAttention(report) {
                        Haptics.success()
                    }
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isProcessing = false
                    isControlsSheetPresented = true
                }
            }
        }
    }

    private func beginImportedImageProcessing() {
        isProcessing = true
        isLiveScanningEnabled = false
        errorMessage = nil
        scanReport = nil
        isControlsSheetPresented = true
    }

    private nonisolated static func readSecurityScopedFileData(from url: URL) throws -> Data {
        let didStartAccessing = url.startAccessingSecurityScopedResource()
        defer {
            if didStartAccessing {
                url.stopAccessingSecurityScopedResource()
            }
        }

        return try Data(contentsOf: url)
    }

    private nonisolated static func makeImportedScanReport(
        from imageData: Data,
        source: OrbScanSource,
        expectedOrbID: String
    ) throws -> OrbScanReport {
        let importedOrbScan = try scanImportedOrb(from: imageData)
        return makeScanReport(
            orbID: importedOrbScan.orbID,
            source: source,
            verificationState: importedOrbScan.verificationState,
            expectedOrbID: expectedOrbID
        )
    }

    private nonisolated static func scanImportedOrb(from imageData: Data) throws -> ImportedOrbScan {
        do {
            let orbID = try OrbCodeKit.scanOrbID(fromImageData: imageData)
            return ImportedOrbScan(
                orbID: orbID,
                verificationState: makeVerificationState(imageData)
            )
        } catch {}

        guard let image = UIImage(data: imageData) else {
            throw OrbScannerImportError.unsupportedImage
        }

        do {
            let normalizedPNGData = try OrbCodeKit.normalizedPNGData(from: image)
            let orbID = try OrbCodeKit.scanOrbID(fromImageData: normalizedPNGData)
            return ImportedOrbScan(
                orbID: orbID,
                verificationState: makeVerificationState(normalizedPNGData)
            )
        } catch {}

        do {
            let orbID = try OrbCodeKit.scanOrbID(fromCameraFrame: image)
            return ImportedOrbScan(
                orbID: orbID,
                verificationState: .imageRecovered(
                    "Looper found the orb after normalizing and cropping the uploaded image."
                )
            )
        } catch {}

        throw OrbScannerImportError.notRecognized
    }

    private func makeLiveScanReport(orbID: String) -> OrbScanReport {
        Self.makeScanReport(
            orbID: orbID,
            source: .camera,
            verificationState: .livePayloadAccepted,
            expectedOrbID: expectedOrbID
        )
    }

    private nonisolated static func makeScanReport(
        orbID: String,
        source: OrbScanSource,
        verificationState: OrbVerificationState,
        expectedOrbID: String
    ) -> OrbScanReport {
        let trimmedExpectedOrbID = expectedOrbID.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedExpectedOrbID = trimmedExpectedOrbID.isEmpty ? nil : trimmedExpectedOrbID

        return OrbScanReport(
            orbID: orbID,
            source: source,
            scannedAt: Date(),
            verificationState: verificationState,
            expectedOrbID: normalizedExpectedOrbID
        )
    }

    private nonisolated static func makeVerificationState(_ imageData: Data) -> OrbVerificationState {
        do {
            let didVerify = try OrbCodeKit.verifyOrb(fromPNG: imageData)
            return didVerify ? .imageVerified : .failed(
                "The orb payload decoded, but visual verification did not pass."
            )
        } catch {
            return .failed(error.localizedDescription)
        }
    }

    private nonisolated static func scanReportNeedsAttention(_ scanReport: OrbScanReport) -> Bool {
        if scanReport.expectationMatches == false {
            return true
        }

        if case .failed = scanReport.verificationState {
            return true
        }

        return false
    }

    private func resumeLiveScanning() {
        scanReport = nil
        errorMessage = nil
        isProcessing = false
        isLiveScanningEnabled = true
        liveCameraDiagnostics = LiveOrbCameraDiagnostics()
        cameraResetToken += 1
    }

    private func openControlsSheet() {
        isControlsSheetPresented = true
    }

    private func resetScanner() {
        expectedOrbID = ""
        selectedPhotoItem = nil
        resumeLiveScanning()
    }

    private func verificationTint(_ state: OrbVerificationState) -> Color {
        switch state {
        case .imageVerified, .imageRecovered, .livePayloadAccepted:
            return .green
        case .failed:
            return .red
        }
    }

    private func expectationTint(_ matches: Bool?) -> Color {
        switch matches {
        case .some(true):
            return .green
        case .some(false):
            return .red
        case .none:
            return .secondary
        }
    }
}

#Preview {
    OrbScannerScreen()
}
