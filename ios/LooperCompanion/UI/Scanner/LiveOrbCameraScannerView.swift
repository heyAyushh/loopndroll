import AVFoundation
import CoreImage
import CoreVideo
import SwiftUI
import UIKit
import Vision

private enum LiveOrbCameraFrameMetrics {
    static let scanInterval: CFTimeInterval = 0.22
    static let diagnosticsPublishFrameInterval = 4
    static let unsetResetToken = Int.min
}

private enum RealtimeOrbScannerMetrics {
    static let visionContrastAdjustment: Float = 2.0
    static let visionRegionOfInterestFraction: CGFloat = 0.7
    static let candidateMinimumAspectRatio: CGFloat = 0.85
    static let candidateMaximumAspectRatio: CGFloat = 1.15
    static let candidateMinimumAreaFractionOfROI: CGFloat = 0.06
    static let candidateMaximumAreaFractionOfROI: CGFloat = 0.80
    static let candidateDecodeAttemptLimit = 2
    static let candidatePaddingFractions: [CGFloat] = [0.10, 0.24]
    static let fallbackCenterCropFractions: [CGFloat] = [0.74, 0.86]
    static let downscaleMaximumLongestEdge: Int = 640
}

enum LiveOrbCameraAvailability: Equatable {
    case ready
    case unavailable(String)
}

enum LiveOrbCameraResetDefaults {
    static let initialToken = 0
}

struct LiveOrbCameraDiagnostics: Equatable, Sendable {
    var analyzedFrameCount = 0
    var lastFailureMessage: String?
    var lastDetectedOrbID: String?

    var hasAnalyzedFrames: Bool {
        analyzedFrameCount > 0
    }
}

struct LiveOrbCameraScannerView: UIViewControllerRepresentable {
    let isScanningEnabled: Bool
    let payloadTransformer: ((String) -> String?)?
    let imageDataTransformer: ((Data) throws -> String)?
    let lumaTransformer: ((Data, UInt32, UInt32) throws -> String)?
    let onValueDetected: (String) -> Void
    let onAvailabilityChange: (LiveOrbCameraAvailability) -> Void
    let onDiagnosticsChange: (LiveOrbCameraDiagnostics) -> Void
    let resetToken: Int

    init(
        isScanningEnabled: Bool,
        payloadTransformer: ((String) -> String?)?,
        imageDataTransformer: ((Data) throws -> String)?,
        lumaTransformer: ((Data, UInt32, UInt32) throws -> String)? = nil,
        onValueDetected: @escaping (String) -> Void,
        onAvailabilityChange: @escaping (LiveOrbCameraAvailability) -> Void,
        onDiagnosticsChange: @escaping (LiveOrbCameraDiagnostics) -> Void,
        resetToken: Int = LiveOrbCameraResetDefaults.initialToken
    ) {
        self.isScanningEnabled = isScanningEnabled
        self.payloadTransformer = payloadTransformer
        self.imageDataTransformer = imageDataTransformer
        self.lumaTransformer = lumaTransformer
        self.onValueDetected = onValueDetected
        self.onAvailabilityChange = onAvailabilityChange
        self.onDiagnosticsChange = onDiagnosticsChange
        self.resetToken = resetToken
    }

    func makeUIViewController(context: Context) -> LiveOrbCameraViewController {
        let controller = LiveOrbCameraViewController()
        controller.payloadTransformer = payloadTransformer
        controller.imageDataTransformer = imageDataTransformer
        controller.lumaTransformer = lumaTransformer
        controller.onValueDetected = onValueDetected
        controller.onAvailabilityChange = onAvailabilityChange
        controller.onDiagnosticsChange = onDiagnosticsChange
        controller.setScanningEnabled(isScanningEnabled, resetToken: resetToken)
        return controller
    }

    func updateUIViewController(
        _ uiViewController: LiveOrbCameraViewController,
        context _: Context
    ) {
        uiViewController.payloadTransformer = payloadTransformer
        uiViewController.imageDataTransformer = imageDataTransformer
        uiViewController.lumaTransformer = lumaTransformer
        uiViewController.onValueDetected = onValueDetected
        uiViewController.onAvailabilityChange = onAvailabilityChange
        uiViewController.onDiagnosticsChange = onDiagnosticsChange
        uiViewController.setScanningEnabled(isScanningEnabled, resetToken: resetToken)
    }
}

final class LiveOrbCameraViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    nonisolated(unsafe) var payloadTransformer: ((String) -> String?)?
    nonisolated(unsafe) var imageDataTransformer: ((Data) throws -> String)?
    nonisolated(unsafe) var lumaTransformer: ((Data, UInt32, UInt32) throws -> String)?
    nonisolated(unsafe) var onValueDetected: ((String) -> Void)?
    nonisolated(unsafe) var onAvailabilityChange: ((LiveOrbCameraAvailability) -> Void)?
    nonisolated(unsafe) var onDiagnosticsChange: ((LiveOrbCameraDiagnostics) -> Void)?

    private nonisolated(unsafe) let captureSession = AVCaptureSession()
    private let captureSessionQueue = DispatchQueue(
        label: "dev.looper.orb-camera.session"
    )
    private let metadataOutputQueue = DispatchQueue(
        label: "dev.looper.orb-camera.metadata-output"
    )
    private let frameOutputQueue = DispatchQueue(
        label: "dev.looper.orb-camera.frame-output"
    )

    private var previewLayer: AVCaptureVideoPreviewLayer?
    private nonisolated(unsafe) var isCaptureSessionConfigured = false
    private nonisolated(unsafe) var isScanningEnabled = true
    private nonisolated(unsafe) var hasAcceptedDetection = false
    private nonisolated(unsafe) var isFrameScanInFlight = false
    private nonisolated(unsafe) var lastFrameScanTime: CFTimeInterval = 0
    private nonisolated(unsafe) var analyzedFrameCount = 0
    private nonisolated(unsafe) var lastFailureMessage: String?
    private nonisolated(unsafe) var lastDetectedOrbID: String?
    private nonisolated(unsafe) var currentResetToken = LiveOrbCameraFrameMetrics.unsetResetToken

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        configureCameraIfNeeded()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        startCaptureSessionIfNeeded()
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        stopCaptureSession()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        previewLayer?.frame = view.bounds
    }

    func setScanningEnabled(_ isScanningEnabled: Bool, resetToken: Int) {
        let didChangeScanningState = self.isScanningEnabled != isScanningEnabled
        let didRequestReset = currentResetToken != resetToken

        self.isScanningEnabled = isScanningEnabled
        currentResetToken = resetToken

        if isScanningEnabled {
            if didChangeScanningState || didRequestReset {
                resetScanState()
            }
            startCaptureSessionIfNeeded()
        } else if didChangeScanningState {
            stopCaptureSession()
        }
    }

    private nonisolated func resetScanState() {
        hasAcceptedDetection = false
        isFrameScanInFlight = false
        lastFrameScanTime = 0
        analyzedFrameCount = 0
        lastFailureMessage = nil
        lastDetectedOrbID = nil
        publishDiagnostics()
    }

    private func configureCameraIfNeeded() {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            configureCaptureSession()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
                DispatchQueue.main.async {
                    guard let self else {
                        return
                    }

                    if granted {
                        self.configureCaptureSession()
                    } else {
                        self.onAvailabilityChange?(
                            .unavailable("Allow camera access to scan the orb live.")
                        )
                    }
                }
            }
        case .denied, .restricted:
            onAvailabilityChange?(.unavailable("Allow camera access to scan the orb live."))
        @unknown default:
            onAvailabilityChange?(.unavailable("Camera access is unavailable on this device."))
        }
    }

    private func configureCaptureSession() {
        captureSessionQueue.async { [weak self] in
            guard let self else {
                return
            }

            guard !isCaptureSessionConfigured else {
                startCaptureSessionIfNeeded()
                return
            }

            guard
                let camera = AVCaptureDevice.default(
                    .builtInWideAngleCamera,
                    for: .video,
                    position: .back
                )
            else {
                DispatchQueue.main.async {
                    self.onAvailabilityChange?(
                        .unavailable("The rear camera is unavailable on this device.")
                    )
                }
                return
            }

            do {
                self.captureSession.beginConfiguration()
                self.captureSession.sessionPreset = .high
                try self.configureCameraDevice(camera)

                let shouldUseMetadataScanner = self.payloadTransformer != nil
                let shouldUseFrameScanner = self.lumaTransformer != nil || self.imageDataTransformer != nil

                guard shouldUseMetadataScanner || shouldUseFrameScanner else {
                    self.captureSession.commitConfiguration()
                    DispatchQueue.main.async {
                        self.onAvailabilityChange?(
                            .unavailable("The scanner is missing its decode configuration.")
                        )
                    }
                    return
                }

                let cameraInput = try AVCaptureDeviceInput(device: camera)
                guard self.captureSession.canAddInput(cameraInput) else {
                    self.captureSession.commitConfiguration()
                    DispatchQueue.main.async {
                        self.onAvailabilityChange?(
                            .unavailable("The camera session could not be configured.")
                        )
                    }
                    return
                }

                self.captureSession.addInput(cameraInput)

                if shouldUseMetadataScanner {
                    try self.configureMetadataOutput()
                }

                if shouldUseFrameScanner {
                    try self.configureFrameOutput()
                }

                self.captureSession.commitConfiguration()
                self.isCaptureSessionConfigured = true

                DispatchQueue.main.async {
                    let previewLayer = AVCaptureVideoPreviewLayer(session: self.captureSession)
                    previewLayer.videoGravity = .resizeAspectFill
                    previewLayer.frame = self.view.bounds
                    Self.applyPortraitOrientation(to: previewLayer.connection, shouldMirrorVideo: false)
                    self.view.layer.addSublayer(previewLayer)
                    self.previewLayer = previewLayer
                    self.onAvailabilityChange?(.ready)
                }

                self.startCaptureSessionIfNeeded()
            } catch {
                self.captureSession.commitConfiguration()
                DispatchQueue.main.async {
                    self.onAvailabilityChange?(
                        .unavailable("The camera could not start: \(error.localizedDescription)")
                    )
                }
            }
        }
    }

    private nonisolated func configureMetadataOutput() throws {
        let metadataOutput = AVCaptureMetadataOutput()
        guard captureSession.canAddOutput(metadataOutput) else {
            throw LiveOrbCameraConfigurationError.outputUnavailable
        }

        captureSession.addOutput(metadataOutput)
        metadataOutput.setMetadataObjectsDelegate(self, queue: metadataOutputQueue)

        guard metadataOutput.availableMetadataObjectTypes.contains(.qr) else {
            throw LiveOrbCameraConfigurationError.qrUnavailable
        }

        metadataOutput.metadataObjectTypes = [.qr]
        Self.applyPortraitOrientation(to: metadataOutput.connection(with: .video), shouldMirrorVideo: false)
    }

    private nonisolated func configureFrameOutput() throws {
        let videoOutput = AVCaptureVideoDataOutput()
        videoOutput.alwaysDiscardsLateVideoFrames = true
        // Bi-planar 4:2:0 full-range: plane 0 is full-resolution 8-bit luma, exactly
        // what the decoder consumes. Avoids any RGB→gray conversion on the camera path.
        videoOutput.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String:
                Int(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange),
        ]

        guard captureSession.canAddOutput(videoOutput) else {
            throw LiveOrbCameraConfigurationError.outputUnavailable
        }

        captureSession.addOutput(videoOutput)
        videoOutput.setSampleBufferDelegate(self, queue: frameOutputQueue)
        Self.applyPortraitOrientation(to: videoOutput.connection(with: .video), shouldMirrorVideo: false)
    }

    private nonisolated func configureCameraDevice(_ camera: AVCaptureDevice) throws {
        try camera.lockForConfiguration()
        defer { camera.unlockForConfiguration() }

        if camera.isFocusModeSupported(.continuousAutoFocus) {
            camera.focusMode = .continuousAutoFocus
        }

        if camera.isAutoFocusRangeRestrictionSupported {
            camera.autoFocusRangeRestriction = .near
        }

        if camera.isExposureModeSupported(.continuousAutoExposure) {
            camera.exposureMode = .continuousAutoExposure
        }

        if camera.isWhiteBalanceModeSupported(.continuousAutoWhiteBalance) {
            camera.whiteBalanceMode = .continuousAutoWhiteBalance
        }
    }

    private nonisolated static func applyPortraitOrientation(
        to connection: AVCaptureConnection?,
        shouldMirrorVideo: Bool
    ) {
        guard let connection else {
            return
        }

        if #available(iOS 17.0, *) {
            let portraitRotationAngle = CGFloat(90)
            if connection.isVideoRotationAngleSupported(portraitRotationAngle) {
                connection.videoRotationAngle = portraitRotationAngle
            }
        } else if connection.isVideoOrientationSupported {
            connection.videoOrientation = .portrait
        }

        if connection.isVideoMirroringSupported {
            if connection.automaticallyAdjustsVideoMirroring {
                connection.automaticallyAdjustsVideoMirroring = false
            }
            connection.isVideoMirrored = shouldMirrorVideo
        }
    }

    private nonisolated func startCaptureSessionIfNeeded() {
        captureSessionQueue.async { [weak self] in
            guard
                let self,
                isCaptureSessionConfigured,
                isScanningEnabled,
                !captureSession.isRunning
            else {
                return
            }

            captureSession.startRunning()
        }
    }

    private nonisolated func stopCaptureSession() {
        captureSessionQueue.async { [weak self] in
            guard let self, captureSession.isRunning else {
                return
            }

            captureSession.stopRunning()
        }
    }

    private nonisolated func acceptDetectedValue(_ detectedValue: String) {
        guard isScanningEnabled, !hasAcceptedDetection else {
            return
        }

        hasAcceptedDetection = true
        isScanningEnabled = false
        isFrameScanInFlight = false
        lastFailureMessage = nil
        lastDetectedOrbID = detectedValue
        publishDiagnostics()
        stopCaptureSession()

        DispatchQueue.main.async { [onValueDetected] in
            onValueDetected?(detectedValue)
        }
    }

    private nonisolated func publishDiagnostics(force: Bool = true) {
        guard
            force ||
                analyzedFrameCount == 1 ||
                analyzedFrameCount.isMultiple(of: LiveOrbCameraFrameMetrics.diagnosticsPublishFrameInterval)
        else {
            return
        }

        let diagnostics = LiveOrbCameraDiagnostics(
            analyzedFrameCount: analyzedFrameCount,
            lastFailureMessage: lastFailureMessage,
            lastDetectedOrbID: lastDetectedOrbID
        )

        DispatchQueue.main.async { [onDiagnosticsChange] in
            onDiagnosticsChange?(diagnostics)
        }
    }

    nonisolated func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard isScanningEnabled, !hasAcceptedDetection else {
            return
        }

        guard
            let codeObject = metadataObjects.first(where: { metadataObject in
                guard let readableCode = metadataObject as? AVMetadataMachineReadableCodeObject else {
                    return false
                }

                return readableCode.type == .qr && readableCode.stringValue != nil
            }) as? AVMetadataMachineReadableCodeObject,
            let payloadText = codeObject.stringValue,
            let transformedValue = payloadTransformer?(payloadText)
        else {
            return
        }

        acceptDetectedValue(transformedValue)
    }
}

// MARK: - Luma frame pixel region

private struct LumaFrameRegion {
    let cropOriginX: Int
    let cropOriginY: Int
    let cropWidth: Int
    let cropHeight: Int
    let targetWidth: Int
    let targetHeight: Int
}

private struct LumaDecodeAttempt {
    let label: String
    let cropRect: CGRect
}

extension LiveOrbCameraViewController: AVCaptureVideoDataOutputSampleBufferDelegate {
    nonisolated func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        guard
            isScanningEnabled,
            !hasAcceptedDetection
        else {
            return
        }

        let lumaTransformer = self.lumaTransformer
        let imageDataTransformer = self.imageDataTransformer

        guard lumaTransformer != nil || imageDataTransformer != nil else {
            return
        }

        let now = CACurrentMediaTime()
        guard
            !isFrameScanInFlight,
            now - lastFrameScanTime >= LiveOrbCameraFrameMetrics.scanInterval
        else {
            return
        }

        isFrameScanInFlight = true
        lastFrameScanTime = now

        defer {
            if !hasAcceptedDetection {
                isFrameScanInFlight = false
            }
        }

        analyzedFrameCount += 1

        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
            lastFailureMessage = "The camera frame had no pixel buffer."
            publishDiagnostics()
            return
        }

        if let lumaTransformer {
            decodeFromLumaPlane(
                pixelBuffer: pixelBuffer,
                lumaTransformer: lumaTransformer
            )
            return
        }

        // imageDataTransformer is the legacy PNG fallback; the live scanner no longer
        // uses it, but the field is preserved so the metadata-only wiring still compiles.
        lastFailureMessage = "The live scanner requires a luma transformer."
        publishDiagnostics()
    }

    private nonisolated func decodeFromLumaPlane(
        pixelBuffer: CVPixelBuffer,
        lumaTransformer: (Data, UInt32, UInt32) throws -> String
    ) {
        // .readOnly avoids invalidating the GPU-side cache the driver keeps for this
        // buffer — we never write back, so read-only lets subsequent consumers reuse it.
        guard CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly) == kCVReturnSuccess else {
            lastFailureMessage = "The camera frame could not be locked for reading."
            publishDiagnostics()
            return
        }
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        let planeIndex = 0
        let lumaWidth = CVPixelBufferGetWidthOfPlane(pixelBuffer, planeIndex)
        let lumaHeight = CVPixelBufferGetHeightOfPlane(pixelBuffer, planeIndex)
        let lumaBytesPerRow = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, planeIndex)

        guard
            lumaWidth > 0,
            lumaHeight > 0,
            let basePointer = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, planeIndex)
        else {
            lastFailureMessage = "The camera luma plane was empty."
            publishDiagnostics()
            return
        }

        let lumaBase = basePointer.assumingMemoryBound(to: UInt8.self)
        let decodeAttempts = makeDecodeAttempts(
            pixelBuffer: pixelBuffer,
            lumaWidth: lumaWidth,
            lumaHeight: lumaHeight
        )
        var lastDecodeFailure = "The live scanner did not find a decodable orb."

        for attempt in decodeAttempts {
            let region = makeFrameRegion(
                cropRect: attempt.cropRect,
                lumaWidth: lumaWidth,
                lumaHeight: lumaHeight
            )

            guard region.targetWidth > 0, region.targetHeight > 0 else {
                lastDecodeFailure = "\(attempt.label): crop was empty."
                continue
            }

            let rawLumaData = extractLumaCrop(
                lumaBase: lumaBase,
                lumaBytesPerRow: lumaBytesPerRow,
                region: region
            )
            let decodeVariants = makeDecodeVariants(from: rawLumaData)

            for variant in decodeVariants {
                do {
                    let detectedValue = try lumaTransformer(
                        variant.data,
                        UInt32(region.targetWidth),
                        UInt32(region.targetHeight)
                    )
                    acceptDetectedValue(detectedValue)
                    return
                } catch {
                    lastDecodeFailure =
                        "\(attempt.label), \(variant.label): \(error.localizedDescription)"
                }
            }
        }

        lastFailureMessage = lastDecodeFailure
        publishDiagnostics(force: false)
    }

    private nonisolated func makeDecodeAttempts(
        pixelBuffer: CVPixelBuffer,
        lumaWidth: Int,
        lumaHeight: Int
    ) -> [LumaDecodeAttempt] {
        let contourAttempts = detectedCropRects(
            pixelBuffer: pixelBuffer,
            lumaWidth: lumaWidth,
            lumaHeight: lumaHeight
        )
            .prefix(RealtimeOrbScannerMetrics.candidateDecodeAttemptLimit)
            .enumerated()
            .flatMap { index, candidateRect in
                RealtimeOrbScannerMetrics.candidatePaddingFractions.map { paddingFraction in
                    LumaDecodeAttempt(
                        label:
                            "Contour crop \(index + 1) (\(Int((paddingFraction * 100).rounded()))% padding)",
                        cropRect: paddedSquareCropRect(
                            around: candidateRect,
                            lumaWidth: CGFloat(lumaWidth),
                            lumaHeight: CGFloat(lumaHeight),
                            paddingFraction: paddingFraction
                        )
                    )
                }
            }

        let fallbackAttempts = RealtimeOrbScannerMetrics.fallbackCenterCropFractions.map { fraction in
            LumaDecodeAttempt(
                label: "Center crop \(Int((fraction * 100).rounded()))%",
                cropRect: fallbackCenterCropRect(
                    lumaWidth: lumaWidth,
                    lumaHeight: lumaHeight,
                    fraction: fraction
                )
            )
        }

        return contourAttempts + fallbackAttempts
    }

    private nonisolated func detectedCropRects(
        pixelBuffer: CVPixelBuffer,
        lumaWidth: Int,
        lumaHeight: Int
    ) -> [CGRect] {
        let request = VNDetectContoursRequest()
        request.contrastAdjustment = RealtimeOrbScannerMetrics.visionContrastAdjustment
        request.detectsDarkOnLight = true
        // Vision ROI is normalized; centering the square mirrors the on-screen reticle.
        let visionRoiInset = (1 - RealtimeOrbScannerMetrics.visionRegionOfInterestFraction) * 0.5
        request.regionOfInterest = CGRect(
            x: visionRoiInset,
            y: visionRoiInset,
            width: RealtimeOrbScannerMetrics.visionRegionOfInterestFraction,
            height: RealtimeOrbScannerMetrics.visionRegionOfInterestFraction
        )

        // The capture connection is already rotated into portrait for the live preview,
        // so Vision should inspect the frame in its upright orientation here.
        let handler = VNImageRequestHandler(
            cvPixelBuffer: pixelBuffer,
            orientation: .up,
            options: [:]
        )
        do {
            try handler.perform([request])
        } catch {
            return []
        }

        guard let observation = request.results?.first else {
            return []
        }

        let width = CGFloat(lumaWidth)
        let height = CGFloat(lumaHeight)
        let roiPixelArea = (width * RealtimeOrbScannerMetrics.visionRegionOfInterestFraction)
            * (height * RealtimeOrbScannerMetrics.visionRegionOfInterestFraction)
        let minimumPixelArea = roiPixelArea * RealtimeOrbScannerMetrics.candidateMinimumAreaFractionOfROI
        let maximumPixelArea = roiPixelArea * RealtimeOrbScannerMetrics.candidateMaximumAreaFractionOfROI

        var candidateRects: [(rect: CGRect, area: CGFloat)] = []

        for contour in observation.topLevelContours {
            let normalizedRect = contour.normalizedPath.boundingBox
            let pixelRect = Self.pixelRect(
                fromVisionNormalizedRect: normalizedRect,
                lumaWidth: width,
                lumaHeight: height
            )

            guard pixelRect.width > 0, pixelRect.height > 0 else {
                continue
            }

            let aspectRatio = pixelRect.width / pixelRect.height
            guard
                aspectRatio >= RealtimeOrbScannerMetrics.candidateMinimumAspectRatio,
                aspectRatio <= RealtimeOrbScannerMetrics.candidateMaximumAspectRatio
            else {
                continue
            }

            let pixelArea = pixelRect.width * pixelRect.height
            guard pixelArea >= minimumPixelArea, pixelArea <= maximumPixelArea else {
                continue
            }

            candidateRects.append((pixelRect, pixelArea))
        }

        return candidateRects
            .sorted { left, right in
                left.area > right.area
            }
            .map { candidate in
                candidate.rect
            }
    }

    private nonisolated static func pixelRect(
        fromVisionNormalizedRect normalizedRect: CGRect,
        lumaWidth: CGFloat,
        lumaHeight: CGFloat
    ) -> CGRect {
        // Vision's normalized coords use bottom-left origin. We're working in top-left
        // pixel coords for the luma buffer, so flip Y here.
        let originX = normalizedRect.minX * lumaWidth
        let flippedOriginY = (1 - normalizedRect.maxY) * lumaHeight
        let width = normalizedRect.width * lumaWidth
        let height = normalizedRect.height * lumaHeight
        return CGRect(x: originX, y: flippedOriginY, width: width, height: height)
    }

    private nonisolated func paddedSquareCropRect(
        around candidateRect: CGRect,
        lumaWidth: CGFloat,
        lumaHeight: CGFloat,
        paddingFraction: CGFloat
    ) -> CGRect {
        let longerEdge = max(candidateRect.width, candidateRect.height)
        let paddedEdge = longerEdge * (1 + paddingFraction * 2)
        let centerX = candidateRect.midX
        let centerY = candidateRect.midY

        var originX = centerX - paddedEdge * 0.5
        var originY = centerY - paddedEdge * 0.5
        var edge = paddedEdge

        edge = min(edge, lumaWidth, lumaHeight)
        originX = max(0, min(originX, lumaWidth - edge))
        originY = max(0, min(originY, lumaHeight - edge))

        return CGRect(x: originX, y: originY, width: edge, height: edge)
    }

    private nonisolated func fallbackCenterCropRect(
        lumaWidth: Int,
        lumaHeight: Int,
        fraction: CGFloat
    ) -> CGRect {
        let width = CGFloat(lumaWidth)
        let height = CGFloat(lumaHeight)
        let edge = min(width, height) * fraction
        let originX = (width - edge) * 0.5
        let originY = (height - edge) * 0.5
        return CGRect(x: originX, y: originY, width: edge, height: edge)
    }

    private nonisolated func makeFrameRegion(
        cropRect: CGRect,
        lumaWidth: Int,
        lumaHeight: Int
    ) -> LumaFrameRegion {
        let clampedOriginX = max(0, Int(cropRect.origin.x.rounded(.down)))
        let clampedOriginY = max(0, Int(cropRect.origin.y.rounded(.down)))
        let requestedWidth = max(0, Int(cropRect.width.rounded(.down)))
        let requestedHeight = max(0, Int(cropRect.height.rounded(.down)))
        let cropWidth = max(0, min(requestedWidth, lumaWidth - clampedOriginX))
        let cropHeight = max(0, min(requestedHeight, lumaHeight - clampedOriginY))

        let longestEdge = max(cropWidth, cropHeight)
        let cap = RealtimeOrbScannerMetrics.downscaleMaximumLongestEdge
        let targetWidth: Int
        let targetHeight: Int
        if longestEdge <= cap || longestEdge == 0 {
            targetWidth = cropWidth
            targetHeight = cropHeight
        } else {
            // Nearest-neighbor subsample at the cap; the Rust decoder resamples to 320
            // internally, so preserving exact edges here is unnecessary.
            targetWidth = max(1, cropWidth * cap / longestEdge)
            targetHeight = max(1, cropHeight * cap / longestEdge)
        }

        return LumaFrameRegion(
            cropOriginX: clampedOriginX,
            cropOriginY: clampedOriginY,
            cropWidth: cropWidth,
            cropHeight: cropHeight,
            targetWidth: targetWidth,
            targetHeight: targetHeight
        )
    }

    private nonisolated func extractLumaCrop(
        lumaBase: UnsafePointer<UInt8>,
        lumaBytesPerRow: Int,
        region: LumaFrameRegion
    ) -> Data {
        // Tightly packed output: one byte per pixel, no stride padding. Nearest-neighbor
        // is sufficient because the Rust decoder downsamples to 320px internally.
        let targetWidth = region.targetWidth
        let targetHeight = region.targetHeight
        var output = Data(count: targetWidth * targetHeight)

        output.withUnsafeMutableBytes { rawBuffer in
            guard let outputBase = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return
            }

            for targetY in 0..<targetHeight {
                let sourceY = region.cropOriginY + targetY * region.cropHeight / targetHeight
                let sourceRow = lumaBase.advanced(by: sourceY * lumaBytesPerRow)
                let outputRow = outputBase.advanced(by: targetY * targetWidth)

                for targetX in 0..<targetWidth {
                    let sourceX = region.cropOriginX + targetX * region.cropWidth / targetWidth
                    outputRow[targetX] = sourceRow[sourceX]
                }
            }
        }

        return output
    }

    private nonisolated func makeDecodeVariants(from lumaData: Data) -> [(label: String, data: Data)] {
        var variants = [(label: String, data: Data)]()
        variants.append((label: "raw", data: lumaData))

        if let stretched = contrastStretchedLumaData(from: lumaData) {
            variants.append((label: "contrast", data: stretched))
        }

        return variants
    }

    private nonisolated func contrastStretchedLumaData(from lumaData: Data) -> Data? {
        guard
            let minimumValue = lumaData.min(),
            let maximumValue = lumaData.max(),
            maximumValue > minimumValue
        else {
            return nil
        }

        let inputRange = Float(maximumValue - minimumValue)
        return Data(lumaData.map { sample in
            let normalized = (Float(sample) - Float(minimumValue)) / inputRange
            let stretched = min(max(normalized, 0), 1) * 255
            return UInt8(stretched.rounded())
        })
    }
}

private enum LiveOrbCameraConfigurationError: LocalizedError {
    case outputUnavailable
    case qrUnavailable

    var errorDescription: String? {
        switch self {
        case .outputUnavailable:
            return "The camera output could not be configured."
        case .qrUnavailable:
            return "QR scanning is unavailable on this device."
        }
    }
}
