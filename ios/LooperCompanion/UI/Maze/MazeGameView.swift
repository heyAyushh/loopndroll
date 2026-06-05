@preconcurrency import CoreMotion
@preconcurrency import SpriteKit
import SwiftUI
import UIKit

struct SettingsMazeScreen: View {
    @Environment(\.dismiss) private var dismiss
    @State private var resetID = UUID()
    @State private var isSceneActive = true

    var body: some View {
        ZStack(alignment: .top) {
            if isSceneActive {
                MazeGameView(resetID: resetID)
                    .ignoresSafeArea()
            }

            HStack {
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "chevron.backward")
                        .font(.headline.weight(.semibold))
                        .frame(width: 44, height: 44)
                }
                .accessibilityLabel("Back")

                Spacer()

                Button {
                    resetID = UUID()
                } label: {
                    Image(systemName: "shuffle")
                        .font(.headline.weight(.semibold))
                        .frame(width: 44, height: 44)
                }
                .accessibilityLabel("Random Maze")
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.regular)
            .tint(.black.opacity(0.62))
            .padding(.horizontal, 14)
            .padding(.top, 8)
        }
        .background(Color(uiColor: .systemGroupedBackground))
        .navigationBarBackButtonHidden(true)
        .toolbar(.hidden, for: .navigationBar)
        .toolbar(.hidden, for: .tabBar)
        .statusBarHidden(true)
        .onAppear {
            isSceneActive = true
        }
        .onDisappear {
            isSceneActive = false
        }
    }
}

struct MazeGameView: UIViewRepresentable {
    @Environment(\.scenePhase) private var scenePhase

    let resetID: UUID

    func makeUIView(context: Context) -> SKView {
        let view = SKView(frame: .zero)
        view.backgroundColor = .clear
        view.allowsTransparency = true
        view.ignoresSiblingOrder = true
        view.preferredFramesPerSecond = 120

        let scene = MazeScene(size: UIScreen.main.bounds.size)
        view.presentScene(scene)
        context.coordinator.scene = scene
        return view
    }

    func updateUIView(_ view: SKView, context: Context) {
        context.coordinator.scene?.size = view.bounds.size
        context.coordinator.scene?.setSceneActive(scenePhase == .active)
        context.coordinator.scene?.setInterfaceStyle(view.traitCollection.userInterfaceStyle)
        context.coordinator.resetIfNeeded(resetID)
    }

    static func dismantleUIView(_ view: SKView, coordinator: Coordinator) {
        coordinator.shutdown()
        view.isPaused = true
        view.presentScene(nil)
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        var scene: MazeScene?
        private var currentResetID: UUID?

        func resetIfNeeded(_ resetID: UUID) {
            guard resetID != currentResetID else {
                return
            }

            currentResetID = resetID
            scene?.restartMaze()
        }

        func shutdown() {
            scene?.shutdown()
            scene = nil
        }
    }
}

private enum MazePhysicsCategory {
    static let ball: UInt32 = 0x1 << 0
    static let wall: UInt32 = 0x1 << 1
    static let goal: UInt32 = 0x1 << 2
}

@MainActor
final class MazeScene: SKScene, @preconcurrency SKPhysicsContactDelegate {
    private enum Layout {
        static let boardInset: CGFloat = 5
        static let ballCollisionRadius: CGFloat = 8
        static let ballVisualRadius: CGFloat = 16
        static let wallLineWidth: CGFloat = 4.8
        static let targetLanePitch: CGFloat = 30
        static let minimumRingCount = 7
        static let maximumRingCount = 8
        static let rectangularCellPitch: CGFloat = 35
        static let minimumColumns = 9
        static let maximumColumns = 16
        static let minimumRows = 14
        static let maximumRows = 30
    }

    private enum Motion {
        static let updateInterval = 1.0 / 120.0
        static let gravityScale: CGFloat = 24
    }

    private let motionManager = CMMotionManager()
    private let hapticManager = PinballHapticManager()
    private let boardRoot = SKNode()
    private var ball: SKNode?
    private var boardRect = CGRect.zero
    private var mazeCenter = CGPoint.zero
    private var outerRadius: CGFloat = 0
    private var currentInterfaceStyle: UIUserInterfaceStyle = .unspecified
    private var mazeKind: MazeKind = .rectangular
    private var maze: GeneratedMaze?
    private var rectangularMaze: RectangularMaze?
    private var mazeSeed: UInt64 = 0
    private var mazeSerial: UInt64 = 0
    private var lastSceneSize = CGSize.zero
    private var hasSolvedMaze = false
    private var isSceneActive = false
    private var isMotionActive = false
    private var isShuttingDown = false
    private var pendingRestart: DispatchWorkItem?

    override init(size: CGSize) {
        super.init(size: size)
        scaleMode = .resizeFill
        backgroundColor = .clear
    }

    required init?(coder aDecoder: NSCoder) {
        super.init(coder: aDecoder)
        scaleMode = .resizeFill
        backgroundColor = .clear
    }

    override func didMove(to view: SKView) {
        isShuttingDown = false
        physicsWorld.contactDelegate = self
        physicsWorld.gravity = CGVector(dx: 0, dy: -9.8)
        addChild(boardRoot)
        rebuildMaze(force: true, regenerate: true)
        setSceneActive(true)
    }

    override func willMove(from view: SKView) {
        shutdown()
    }

    override func didChangeSize(_ oldSize: CGSize) {
        super.didChangeSize(oldSize)
        rebuildMaze(force: true)
    }

    override func update(_ currentTime: TimeInterval) {
        guard !isShuttingDown else {
            return
        }

        rebuildMaze()
        keepBallInBoard()
    }

    func didBegin(_ contact: SKPhysicsContact) {
        guard isSceneActive, !isShuttingDown else {
            return
        }

        if isGoalContact(contact) {
            completeMaze()
            return
        }

        guard isWallContact(contact) else {
            return
        }

        hapticManager.playCollision(impulse: contact.collisionImpulse, sharpness: 0.68)
    }

    func restartMaze() {
        guard isSceneActive, !isShuttingDown else {
            return
        }

        guard size.width > 20, size.height > 20 else {
            return
        }

        hasSolvedMaze = false
        ball?.removeFromParent()
        ball = nil
        rebuildMaze(force: true, regenerate: true)
        hapticManager.prepare()
    }

    func setInterfaceStyle(_ style: UIUserInterfaceStyle) {
        guard currentInterfaceStyle != style else {
            return
        }

        currentInterfaceStyle = style
        rebuildMaze(force: true)
    }

    func setSceneActive(_ isActive: Bool) {
        guard !isShuttingDown else {
            return
        }

        let wasSceneActive = isSceneActive
        isSceneActive = isActive
        isPaused = !isActive

        if isActive {
            physicsWorld.contactDelegate = self
            if !wasSceneActive {
                hapticManager.prepare()
                startMotion()
                if hasSolvedMaze {
                    restartMaze()
                } else {
                    rebuildMaze(force: true)
                }
            }
        } else {
            guard wasSceneActive else {
                return
            }

            stopMotion()
            physicsWorld.contactDelegate = nil
        }
    }

    func stopMotion() {
        isMotionActive = false
        motionManager.stopDeviceMotionUpdates()
        hapticManager.suspend()
        physicsWorld.gravity = .zero
    }

    func shutdown() {
        guard !isShuttingDown else {
            return
        }

        isShuttingDown = true
        isSceneActive = false
        pendingRestart?.cancel()
        pendingRestart = nil
        isPaused = true
        stopMotion()
        physicsWorld.contactDelegate = nil
        physicsWorld.gravity = .zero
        ball?.removeAllActions()
        ball?.physicsBody = nil
        ball?.removeFromParent()
        ball = nil
        boardRoot.removeAllActions()
        boardRoot.removeAllChildren()
        removeAllActions()
        removeAllChildren()
        maze = nil
        rectangularMaze = nil
        lastSceneSize = .zero
        hasSolvedMaze = false
    }

    private func rebuildMaze(force: Bool = false, regenerate: Bool = false) {
        guard isSceneActive, !isShuttingDown else {
            return
        }

        guard size.width > 20, size.height > 20 else {
            return
        }

        guard force || lastSceneSize != size else {
            return
        }

        if regenerate || mazeSeed == 0 {
            mazeSeed = nextMazeSeed()
            var random = SeededRandomNumberGenerator(seed: mazeSeed)
            mazeKind = MazeKind.random(using: &random)
            maze = nil
            rectangularMaze = nil
        }

        lastSceneSize = size
        boardRoot.removeAllChildren()

        switch mazeKind {
        case .circular:
            boardRect = makeCircularBoardRect()
            mazeCenter = CGPoint(x: boardRect.midX, y: boardRect.midY)
            outerRadius = min(boardRect.width, boardRect.height) / 2
            let ringCount = ringCount(for: outerRadius)
            let shouldRegenerate = regenerate || maze == nil || maze?.ringCount != ringCount || rectangularMaze != nil
            rectangularMaze = nil

            if shouldRegenerate {
                generateCircularMaze(ringCount: ringCount)
            }

        case .rectangular:
            boardRect = makeFullScreenBoardRect()
            mazeCenter = CGPoint(x: boardRect.midX, y: boardRect.midY)
            outerRadius = min(boardRect.width, boardRect.height) / 2
            let dimensions = rectangularDimensions(for: boardRect)
            let shouldRegenerate = regenerate
                || rectangularMaze == nil
                || rectangularMaze?.columns != dimensions.columns
                || rectangularMaze?.rows != dimensions.rows
                || maze != nil
            maze = nil

            if shouldRegenerate {
                generateRectangularMaze(columns: dimensions.columns, rows: dimensions.rows)
            }
        }

        addBoard()
        addMazeWalls()
        addGoal()

        if ball == nil {
            addBall()
        } else {
            ball?.position = constrainedPosition(ball?.position ?? startPosition())
        }
    }

    private func generateCircularMaze(ringCount: Int) {
        var generatedMaze = MazeGenerator.generate(ringCount: ringCount, seed: mazeSeed)
        var attempts = 0
        while !generatedMaze.hasCenterSolution && attempts < 4 {
            mazeSeed = nextMazeSeed()
            generatedMaze = MazeGenerator.generate(ringCount: ringCount, seed: mazeSeed)
            attempts += 1
        }
        maze = generatedMaze
    }

    private func generateRectangularMaze(columns: Int, rows: Int) {
        var generatedMaze = RectangularMazeGenerator.generate(columns: columns, rows: rows, seed: mazeSeed)
        var attempts = 0
        while !generatedMaze.hasSolution && attempts < 4 {
            mazeSeed = nextMazeSeed()
            generatedMaze = RectangularMazeGenerator.generate(columns: columns, rows: rows, seed: mazeSeed)
            attempts += 1
        }
        rectangularMaze = generatedMaze
    }

    private func makeCircularBoardRect() -> CGRect {
        let side = max(min(size.width, size.height) - Layout.boardInset * 2, 20)
        return CGRect(
            x: (size.width - side) / 2,
            y: (size.height - side) / 2,
            width: side,
            height: side
        )
    }

    private func makeFullScreenBoardRect() -> CGRect {
        CGRect(
            x: Layout.boardInset,
            y: Layout.boardInset,
            width: max(size.width - Layout.boardInset * 2, 20),
            height: max(size.height - Layout.boardInset * 2, 20)
        )
    }

    private func rectangularDimensions(for rect: CGRect) -> (columns: Int, rows: Int) {
        let measuredColumns = Int(floor(rect.width / Layout.rectangularCellPitch))
        let measuredRows = Int(floor(rect.height / Layout.rectangularCellPitch))
        let columns = min(max(measuredColumns, Layout.minimumColumns), Layout.maximumColumns)
        let rows = min(max(measuredRows, Layout.minimumRows), Layout.maximumRows)
        return (columns, rows)
    }

    private func ringCount(for radius: CGFloat) -> Int {
        let measuredCount = Int(floor(radius / Layout.targetLanePitch))
        return min(max(measuredCount, Layout.minimumRingCount), Layout.maximumRingCount)
    }

    private func nextMazeSeed() -> UInt64 {
        mazeSerial &+= 1
        return 0x9E3779B97F4A7C15 ^ mazeSerial &* 0xBF58476D1CE4E5B9
    }

    private func addBoard() {
        switch mazeKind {
        case .circular:
            addCircularBoard()
        case .rectangular:
            addRectangularBoard()
        }
    }

    private func addCircularBoard() {
        let palette = mazePalette()
        let board = SKShapeNode(circleOfRadius: outerRadius)
        board.position = mazeCenter
        board.fillColor = palette.boardFill
        board.strokeColor = palette.boardStroke
        board.lineWidth = 1
        board.zPosition = 0
        boardRoot.addChild(board)
    }

    private func addRectangularBoard() {
        let palette = mazePalette()
        let board = SKShapeNode(rect: boardRect)
        board.fillColor = palette.boardFill
        board.strokeColor = palette.boardStroke
        board.lineWidth = 1
        board.zPosition = 0
        boardRoot.addChild(board)
    }

    private func addMazeWalls() {
        switch mazeKind {
        case .circular:
            guard let maze else {
                return
            }

            addOuterBoundary()
            for ring in 1..<maze.ringCount {
                addMergedInnerArcs(forRing: ring, in: maze)
            }
            for cell in maze.cells where cell.ring > 0 {
                addClockwiseRadialWallIfClosed(for: cell, in: maze)
            }

        case .rectangular:
            guard let rectangularMaze else {
                return
            }

            addRectangularWalls(in: rectangularMaze)
        }
    }

    private func addOuterBoundary() {
        let path = CGMutablePath()
        path.addArc(
            center: mazeCenter,
            radius: outerRadius,
            startAngle: 0,
            endAngle: CGFloat.pi * 2,
            clockwise: false
        )
        path.closeSubpath()
        addWall(path: path)
    }

    private func addMergedInnerArcs(forRing ring: Int, in maze: GeneratedMaze) {
        let count = maze.cellCount(inRing: ring)
        let closed = (0..<count).map { index in
            let cell = MazeCell(ring: ring, index: index)
            return !maze.hasConnection(between: cell, and: maze.inwardNeighbor(for: cell))
        }

        if closed.allSatisfy({ $0 }) {
            addInnerArc(startingAt: MazeCell(ring: ring, index: 0), length: count, in: maze)
            return
        }

        guard let firstOpen = closed.firstIndex(of: false) else {
            return
        }

        var cursor = (firstOpen + 1) % count
        var scanned = 0

        while scanned < count {
            guard closed[cursor] else {
                cursor = (cursor + 1) % count
                scanned += 1
                continue
            }

            let start = cursor
            var length = 0
            while scanned < count, closed[cursor] {
                length += 1
                cursor = (cursor + 1) % count
                scanned += 1
            }

            addInnerArc(startingAt: MazeCell(ring: ring, index: start), length: length, in: maze)
        }
    }

    private func addInnerArc(startingAt cell: MazeCell, length: Int, in maze: GeneratedMaze) {
        guard length > 0 else {
            return
        }

        let geometry = cellGeometry(for: cell, in: maze)
        let cellCount = CGFloat(maze.cellCount(inRing: cell.ring))
        let angleSpan = CGFloat.pi * 2 / cellCount
        let endAngle = geometry.startAngle + angleSpan * CGFloat(length)
        let path = CGMutablePath()
        path.addArc(
            center: mazeCenter,
            radius: geometry.innerRadius,
            startAngle: geometry.startAngle,
            endAngle: endAngle,
            clockwise: false
        )

        if length == Int(cellCount) {
            path.closeSubpath()
        }

        addWall(path: path)
    }

    private func addClockwiseRadialWallIfClosed(for cell: MazeCell, in maze: GeneratedMaze) {
        let clockwise = maze.clockwiseNeighbor(for: cell)
        guard !maze.hasConnection(between: cell, and: clockwise) else {
            return
        }

        let geometry = cellGeometry(for: cell, in: maze)
        let path = CGMutablePath()
        path.move(to: polarPoint(radius: geometry.innerRadius, angle: geometry.endAngle))
        path.addLine(to: polarPoint(radius: geometry.outerRadius, angle: geometry.endAngle))
        addWall(path: path)
    }

    private func addRectangularWalls(in maze: RectangularMaze) {
        let walls = rectangularWallGrid(for: maze)
        addMergedVerticalWalls(walls.vertical, in: maze)
        addMergedHorizontalWalls(walls.horizontal, in: maze)
    }

    private func rectangularWallGrid(for maze: RectangularMaze) -> (
        vertical: [[Bool]],
        horizontal: [[Bool]]
    ) {
        var vertical = Array(
            repeating: Array(repeating: false, count: maze.columns + 1),
            count: maze.rows
        )
        var horizontal = Array(
            repeating: Array(repeating: false, count: maze.columns),
            count: maze.rows + 1
        )

        for row in 0..<maze.rows {
            vertical[row][0] = true
            vertical[row][maze.columns] = true
        }

        for column in 0..<maze.columns {
            horizontal[0][column] = true
            horizontal[maze.rows][column] = true
        }

        for row in 0..<maze.rows {
            for column in 1..<maze.columns {
                let left = RectCell(row: row, column: column - 1)
                let right = RectCell(row: row, column: column)
                vertical[row][column] = !maze.hasConnection(between: left, and: right)
            }
        }

        for row in 1..<maze.rows {
            for column in 0..<maze.columns {
                let lower = RectCell(row: row - 1, column: column)
                let upper = RectCell(row: row, column: column)
                horizontal[row][column] = !maze.hasConnection(between: lower, and: upper)
            }
        }

        return (vertical, horizontal)
    }

    private func addMergedVerticalWalls(_ walls: [[Bool]], in maze: RectangularMaze) {
        let cellWidth = boardRect.width / CGFloat(maze.columns)
        let cellHeight = boardRect.height / CGFloat(maze.rows)

        for column in 0...maze.columns {
            var row = 0
            while row < maze.rows {
                guard walls[row][column] else {
                    row += 1
                    continue
                }

                let startRow = row
                while row < maze.rows, walls[row][column] {
                    row += 1
                }

                let x = boardRect.minX + CGFloat(column) * cellWidth
                let start = CGPoint(x: x, y: boardRect.minY + CGFloat(startRow) * cellHeight)
                let end = CGPoint(x: x, y: boardRect.minY + CGFloat(row) * cellHeight)
                addRectangularWall(from: start, to: end)
            }
        }
    }

    private func addMergedHorizontalWalls(_ walls: [[Bool]], in maze: RectangularMaze) {
        let cellWidth = boardRect.width / CGFloat(maze.columns)
        let cellHeight = boardRect.height / CGFloat(maze.rows)

        for row in 0...maze.rows {
            var column = 0
            while column < maze.columns {
                guard walls[row][column] else {
                    column += 1
                    continue
                }

                let startColumn = column
                while column < maze.columns, walls[row][column] {
                    column += 1
                }

                let y = boardRect.minY + CGFloat(row) * cellHeight
                let start = CGPoint(x: boardRect.minX + CGFloat(startColumn) * cellWidth, y: y)
                let end = CGPoint(x: boardRect.minX + CGFloat(column) * cellWidth, y: y)
                addRectangularWall(from: start, to: end)
            }
        }
    }

    private func addRectangularWall(from start: CGPoint, to end: CGPoint) {
        let path = CGMutablePath()
        path.move(to: start)
        path.addLine(to: end)
        addWall(path: path)
    }

    private func addWall(path: CGPath) {
        let node = SKShapeNode(path: path)
        node.strokeColor = mazePalette().wallStroke
        node.lineWidth = Layout.wallLineWidth
        node.lineCap = .round
        node.lineJoin = .round
        node.zPosition = 4
        node.physicsBody = SKPhysicsBody(edgeChainFrom: path)
        node.physicsBody?.isDynamic = false
        node.physicsBody?.categoryBitMask = MazePhysicsCategory.wall
        node.physicsBody?.collisionBitMask = MazePhysicsCategory.ball
        node.physicsBody?.contactTestBitMask = MazePhysicsCategory.ball
        node.physicsBody?.restitution = 0.42
        node.physicsBody?.friction = 0.28
        boardRoot.addChild(node)
    }

    private func mazePalette() -> MazePalette {
        let traits = view?.traitCollection ?? UIScreen.main.traitCollection
        let isDark = traits.userInterfaceStyle == .dark
        return MazePalette(
            boardFill: UIColor.secondarySystemGroupedBackground.resolvedColor(with: traits),
            boardStroke: UIColor.separator.resolvedColor(with: traits).withAlphaComponent(0.7),
            wallStroke: UIColor.label.resolvedColor(with: traits).withAlphaComponent(isDark ? 0.82 : 0.9)
        )
    }

    private func addGoal() {
        let goalRadius = Layout.ballVisualRadius * 0.92
        let goal = SKShapeNode(circleOfRadius: goalRadius)
        goal.position = goalPosition()
        goal.fillColor = UIColor.systemGreen.withAlphaComponent(0.22)
        goal.strokeColor = UIColor.systemGreen
        goal.lineWidth = 2
        goal.glowWidth = 2
        goal.zPosition = 2
        goal.physicsBody = SKPhysicsBody(circleOfRadius: goalRadius)
        goal.physicsBody?.isDynamic = false
        goal.physicsBody?.categoryBitMask = MazePhysicsCategory.goal
        goal.physicsBody?.collisionBitMask = 0
        goal.physicsBody?.contactTestBitMask = MazePhysicsCategory.ball
        boardRoot.addChild(goal)
    }

    private func addBall() {
        let node = SKNode()
        let diameter = Layout.ballVisualRadius * 2
        let sprite = SKSpriteNode(
            texture: makeMazeBallTexture(),
            size: CGSize(width: diameter, height: diameter)
        )
        sprite.zPosition = 1

        node.name = "maze-ball"
        node.position = startPosition()
        node.zPosition = 12
        node.addChild(sprite)
        node.physicsBody = SKPhysicsBody(circleOfRadius: Layout.ballCollisionRadius)
        node.physicsBody?.categoryBitMask = MazePhysicsCategory.ball
        node.physicsBody?.collisionBitMask = MazePhysicsCategory.wall
        node.physicsBody?.contactTestBitMask = MazePhysicsCategory.wall | MazePhysicsCategory.goal
        node.physicsBody?.restitution = 0.32
        node.physicsBody?.friction = 0.1
        node.physicsBody?.linearDamping = 0.34
        node.physicsBody?.angularDamping = 0.42
        node.physicsBody?.allowsRotation = true
        addChild(node)
        ball = node
    }

    private func makeMazeBallTexture() -> SKTexture {
        let texture = SKTexture(imageNamed: "MazeBall")
        texture.filteringMode = .linear
        return texture
    }

    private func startMotion() {
        guard !isMotionActive else {
            return
        }

        motionManager.deviceMotionUpdateInterval = Motion.updateInterval
        guard motionManager.isDeviceMotionAvailable else {
            physicsWorld.gravity = CGVector(dx: 0, dy: -9.8)
            return
        }

        isMotionActive = true
        motionManager.startDeviceMotionUpdates(to: .main) { [weak self] motion, _ in
            guard let self, let motion else {
                return
            }
            guard isSceneActive, !isShuttingDown, isMotionActive else {
                return
            }

            physicsWorld.gravity = CGVector(
                dx: motion.gravity.x * Motion.gravityScale,
                dy: motion.gravity.y * Motion.gravityScale
            )
        }
    }

    private func isGoalContact(_ contact: SKPhysicsContact) -> Bool {
        let mask = contact.bodyA.categoryBitMask | contact.bodyB.categoryBitMask
        return mask & MazePhysicsCategory.ball != 0 && mask & MazePhysicsCategory.goal != 0
    }

    private func isWallContact(_ contact: SKPhysicsContact) -> Bool {
        let mask = contact.bodyA.categoryBitMask | contact.bodyB.categoryBitMask
        return mask & MazePhysicsCategory.ball != 0 && mask & MazePhysicsCategory.wall != 0
    }

    private func completeMaze() {
        guard !hasSolvedMaze else {
            return
        }

        hasSolvedMaze = true
        Haptics.success()
        ball?.physicsBody?.isDynamic = false
        ball?.run(.scale(to: 1.18, duration: 0.12))

        let restartWorkItem = DispatchWorkItem { [weak self] in
            Task { @MainActor in
                self?.restartMaze()
            }
        }
        pendingRestart = restartWorkItem
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.18, execute: restartWorkItem)
    }

    private func keepBallInBoard() {
        guard let ball else {
            return
        }

        let nextPosition = constrainedPosition(ball.position)
        if nextPosition != ball.position {
            ball.position = nextPosition
            ball.physicsBody?.velocity = .zero
        }
    }

    private func constrainedPosition(_ point: CGPoint) -> CGPoint {
        switch mazeKind {
        case .circular:
            return constrainedCircularPosition(point)
        case .rectangular:
            return constrainedRectangularPosition(point)
        }
    }

    private func constrainedCircularPosition(_ point: CGPoint) -> CGPoint {
        let maximumDistance = max(outerRadius - Layout.ballCollisionRadius - 5, 1)
        let offset = CGVector(dx: point.x - mazeCenter.x, dy: point.y - mazeCenter.y)
        let distance = hypot(offset.dx, offset.dy)
        guard distance > maximumDistance else {
            return point
        }

        let scale = maximumDistance / distance
        return CGPoint(
            x: mazeCenter.x + offset.dx * scale,
            y: mazeCenter.y + offset.dy * scale
        )
    }

    private func constrainedRectangularPosition(_ point: CGPoint) -> CGPoint {
        let inset = Layout.ballCollisionRadius + Layout.wallLineWidth
        return CGPoint(
            x: min(max(point.x, boardRect.minX + inset), boardRect.maxX - inset),
            y: min(max(point.y, boardRect.minY + inset), boardRect.maxY - inset)
        )
    }

    private func startPosition() -> CGPoint {
        switch mazeKind {
        case .circular:
            guard let maze else {
                return mazeCenter
            }

            return cellCenter(maze.start, in: maze)

        case .rectangular:
            guard let rectangularMaze else {
                return mazeCenter
            }

            return rectCellCenter(rectangularMaze.start, in: rectangularMaze)
        }
    }

    private func goalPosition() -> CGPoint {
        switch mazeKind {
        case .circular:
            guard let maze else {
                return mazeCenter
            }

            return cellCenter(maze.goal, in: maze)

        case .rectangular:
            guard let rectangularMaze else {
                return mazeCenter
            }

            return rectCellCenter(rectangularMaze.goal, in: rectangularMaze)
        }
    }

    private func rectCellCenter(_ cell: RectCell, in maze: RectangularMaze) -> CGPoint {
        let cellWidth = boardRect.width / CGFloat(maze.columns)
        let cellHeight = boardRect.height / CGFloat(maze.rows)
        return CGPoint(
            x: boardRect.minX + (CGFloat(cell.column) + 0.5) * cellWidth,
            y: boardRect.minY + (CGFloat(cell.row) + 0.5) * cellHeight
        )
    }

    private func cellCenter(_ cell: MazeCell, in maze: GeneratedMaze) -> CGPoint {
        guard cell.ring > 0 else {
            return mazeCenter
        }

        let geometry = cellGeometry(for: cell, in: maze)
        return polarPoint(
            radius: (geometry.innerRadius + geometry.outerRadius) / 2,
            angle: (geometry.startAngle + geometry.endAngle) / 2
        )
    }

    private func cellGeometry(for cell: MazeCell, in maze: GeneratedMaze) -> PolarCellGeometry {
        let cellCount = CGFloat(maze.cellCount(inRing: cell.ring))
        let angleSpan = CGFloat.pi * 2 / cellCount
        let startAngle = -CGFloat.pi / 2 + CGFloat(cell.index) * angleSpan
        let innerRadius = CGFloat(cell.ring) * outerRadius / CGFloat(maze.ringCount)
        let outerRadius = CGFloat(cell.ring + 1) * outerRadius / CGFloat(maze.ringCount)
        return PolarCellGeometry(
            innerRadius: innerRadius,
            outerRadius: outerRadius,
            startAngle: startAngle,
            endAngle: startAngle + angleSpan
        )
    }

    private func polarPoint(radius: CGFloat, angle: CGFloat) -> CGPoint {
        CGPoint(
            x: mazeCenter.x + cos(angle) * radius,
            y: mazeCenter.y + sin(angle) * radius
        )
    }
}

private struct PolarCellGeometry {
    let innerRadius: CGFloat
    let outerRadius: CGFloat
    let startAngle: CGFloat
    let endAngle: CGFloat
}

private struct MazePalette {
    let boardFill: UIColor
    let boardStroke: UIColor
    let wallStroke: UIColor
}
