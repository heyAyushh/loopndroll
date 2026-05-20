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
                    Image(systemName: "arrow.counterclockwise")
                        .font(.headline.weight(.semibold))
                        .frame(width: 44, height: 44)
                }
                .accessibilityLabel("Reset Maze")
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
    private var maze: GeneratedMaze?
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

        lastSceneSize = size
        boardRoot.removeAllChildren()
        boardRect = makeBoardRect()
        mazeCenter = CGPoint(x: boardRect.midX, y: boardRect.midY)
        outerRadius = min(boardRect.width, boardRect.height) / 2
        let ringCount = ringCount(for: outerRadius)
        let shouldRegenerate = regenerate || maze == nil || maze?.ringCount != ringCount

        if shouldRegenerate {
            if regenerate || mazeSeed == 0 {
                mazeSeed = nextMazeSeed()
            }
            var generatedMaze = MazeGenerator.generate(ringCount: ringCount, seed: mazeSeed)
            var attempts = 0
            while !generatedMaze.hasCenterSolution && attempts < 3 {
                mazeSeed = nextMazeSeed()
                generatedMaze = MazeGenerator.generate(ringCount: ringCount, seed: mazeSeed)
                attempts += 1
            }
            maze = generatedMaze
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

    private func makeBoardRect() -> CGRect {
        let side = max(min(size.width, size.height) - Layout.boardInset * 2, 20)
        return CGRect(
            x: (size.width - side) / 2,
            y: (size.height - side) / 2,
            width: side,
            height: side
        )
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
        let board = SKShapeNode(circleOfRadius: outerRadius)
        board.position = mazeCenter
        board.fillColor = UIColor.secondarySystemGroupedBackground
        board.strokeColor = UIColor.separator.withAlphaComponent(0.7)
        board.lineWidth = 1
        board.zPosition = 0
        boardRoot.addChild(board)
    }

    private func addMazeWalls() {
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

    private func addWall(path: CGPath) {
        let node = SKShapeNode(path: path)
        node.strokeColor = UIColor.black.withAlphaComponent(0.9)
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

    private func startPosition() -> CGPoint {
        guard let maze else {
            return mazeCenter
        }

        return cellCenter(maze.start, in: maze)
    }

    private func goalPosition() -> CGPoint {
        guard let maze else {
            return mazeCenter
        }

        return cellCenter(maze.goal, in: maze)
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

private struct MazeCell: Hashable {
    let ring: Int
    let index: Int
}

private struct GeneratedMaze {
    let ringCellCounts: [Int]
    let start: MazeCell
    let goal: MazeCell
    let connections: Set<MazeConnection>
    let solutionPath: [MazeCell]

    var ringCount: Int {
        ringCellCounts.count
    }

    var hasCenterSolution: Bool {
        solutionPath.first == start && solutionPath.last == goal
    }

    var cells: [MazeCell] {
        ringCellCounts.enumerated().flatMap { ring, count in
            (0..<count).map { MazeCell(ring: ring, index: $0) }
        }
    }

    func cellCount(inRing ring: Int) -> Int {
        ringCellCounts[ring]
    }

    func hasConnection(between first: MazeCell, and second: MazeCell) -> Bool {
        connections.contains(MazeConnection(first, second))
    }

    func clockwiseNeighbor(for cell: MazeCell) -> MazeCell {
        let count = cellCount(inRing: cell.ring)
        return MazeCell(ring: cell.ring, index: (cell.index + 1) % count)
    }

    func inwardNeighbor(for cell: MazeCell) -> MazeCell {
        guard cell.ring > 0 else {
            return cell
        }

        let innerCount = cellCount(inRing: cell.ring - 1)
        let currentCount = cellCount(inRing: cell.ring)
        let innerIndex = min(Int(CGFloat(cell.index) * CGFloat(innerCount) / CGFloat(currentCount)), innerCount - 1)
        return MazeCell(ring: cell.ring - 1, index: innerIndex)
    }
}

private struct MazeConnection: Hashable {
    let first: MazeCell
    let second: MazeCell

    init(_ first: MazeCell, _ second: MazeCell) {
        if first.ring < second.ring || first.ring == second.ring && first.index <= second.index {
            self.first = first
            self.second = second
        } else {
            self.first = second
            self.second = first
        }
    }
}

private enum MazeGenerator {
    static func generate(ringCount: Int, seed: UInt64) -> GeneratedMaze {
        var random = SeededRandomNumberGenerator(seed: seed)
        let ringCellCounts = ringCellCounts(for: ringCount)
        let start = MazeCell(ring: ringCount - 1, index: 0)
        let goal = MazeCell(ring: 0, index: 0)
        let topology = GeneratedMaze(
            ringCellCounts: ringCellCounts,
            start: start,
            goal: goal,
            connections: [],
            solutionPath: []
        )
        let connections = carvePerfectMaze(from: start, in: topology, using: &random)
        let solutionPath = path(from: start, to: goal, connections: connections, in: topology)

        return GeneratedMaze(
            ringCellCounts: ringCellCounts,
            start: start,
            goal: goal,
            connections: connections,
            solutionPath: solutionPath
        )
    }

    private static func ringCellCounts(for ringCount: Int) -> [Int] {
        guard ringCount > 1 else {
            return [1]
        }

        var counts = [1]
        for ring in 1..<ringCount {
            if ring == 1 {
                counts.append(6)
                continue
            }

            let previousCount = counts[ring - 1]
            let ringRadius = Double(ring) / Double(ringCount)
            let rowHeight = 1.0 / Double(ringCount)
            let previousCellWidth = 2 * Double.pi * ringRadius / Double(previousCount)
            let splitRatio = min(max(Int((previousCellWidth / rowHeight).rounded()), 1), 2)
            counts.append(min(previousCount * splitRatio, 48))
        }

        return counts
    }

    private static func carvePerfectMaze(
        from start: MazeCell,
        in maze: GeneratedMaze,
        using random: inout SeededRandomNumberGenerator
    ) -> Set<MazeConnection> {
        var visited: Set<MazeCell> = [start]
        var active = [start]
        var connections: Set<MazeConnection> = []
        let cellTotal = maze.cells.count

        while visited.count < cellTotal {
            guard let index = activeCellIndex(from: active, in: maze, visited: visited, using: &random) else {
                break
            }

            let current = active[index]
            let neighbors = unvisitedNeighbors(from: current, in: maze, visited: visited)
            guard let next = chooseNeighbor(from: neighbors, current: current, using: &random) else {
                active.remove(at: index)
                continue
            }

            connections.insert(MazeConnection(current, next))
            visited.insert(next)
            active.append(next)
        }

        return connections
    }

    private static func activeCellIndex(
        from active: [MazeCell],
        in maze: GeneratedMaze,
        visited: Set<MazeCell>,
        using random: inout SeededRandomNumberGenerator
    ) -> Int? {
        guard !active.isEmpty else {
            return nil
        }

        if random.nextInt(in: 0...99) < 72 {
            return active.indices.reversed().first {
                !unvisitedNeighbors(from: active[$0], in: maze, visited: visited).isEmpty
            }
        }

        return active.indices.filter {
            !unvisitedNeighbors(from: active[$0], in: maze, visited: visited).isEmpty
        }.randomElement(using: &random)
    }

    private static func chooseNeighbor(
        from neighbors: [MazeCell],
        current: MazeCell,
        using random: inout SeededRandomNumberGenerator
    ) -> MazeCell? {
        guard !neighbors.isEmpty else {
            return nil
        }

        let weighted = neighbors.flatMap { neighbor -> [MazeCell] in
            if neighbor.ring == current.ring {
                return Array(repeating: neighbor, count: 2)
            }

            if neighbor.ring < current.ring {
                return Array(repeating: neighbor, count: 3)
            }

            return [neighbor]
        }

        return weighted.randomElement(using: &random)
    }

    private static func path(
        from start: MazeCell,
        to goal: MazeCell,
        connections: Set<MazeConnection>,
        in maze: GeneratedMaze
    ) -> [MazeCell] {
        var frontier = [start]
        var cameFrom: [MazeCell: MazeCell] = [:]
        var visited: Set<MazeCell> = [start]

        while let current = frontier.first {
            frontier.removeFirst()
            if current == goal {
                break
            }

            for neighbor in connectedNeighbors(of: current, connections: connections, in: maze) where !visited.contains(neighbor) {
                visited.insert(neighbor)
                cameFrom[neighbor] = current
                frontier.append(neighbor)
            }
        }

        var path = [goal]
        var current = goal
        while current != start, let previous = cameFrom[current] {
            path.append(previous)
            current = previous
        }
        return path.reversed()
    }

    private static func connectedNeighbors(
        of cell: MazeCell,
        connections: Set<MazeConnection>,
        in maze: GeneratedMaze
    ) -> [MazeCell] {
        neighbors(from: cell, in: maze).filter {
            connections.contains(MazeConnection(cell, $0))
        }
    }

    private static func unvisitedNeighbors(
        from cell: MazeCell,
        in maze: GeneratedMaze,
        visited: Set<MazeCell>
    ) -> [MazeCell] {
        neighbors(from: cell, in: maze).filter { !visited.contains($0) }
    }

    private static func neighbors(from cell: MazeCell, in maze: GeneratedMaze) -> [MazeCell] {
        var neighbors: [MazeCell] = []

        if cell.ring > 0 {
            neighbors.append(neighbor(onSameRingFrom: cell, direction: 1, in: maze))
            neighbors.append(neighbor(onSameRingFrom: cell, direction: -1, in: maze))
            neighbors.append(maze.inwardNeighbor(for: cell))
        }

        if cell.ring < maze.ringCount - 1 {
            let outwardRing = cell.ring + 1
            let outwardCount = maze.cellCount(inRing: outwardRing)
            for outwardIndex in 0..<outwardCount {
                let outwardCell = MazeCell(ring: outwardRing, index: outwardIndex)
                if maze.inwardNeighbor(for: outwardCell) == cell {
                    neighbors.append(outwardCell)
                }
            }
        }

        return neighbors
    }

    private static func neighbor(onSameRingFrom cell: MazeCell, direction: Int, in maze: GeneratedMaze) -> MazeCell {
        let count = maze.cellCount(inRing: cell.ring)
        let index = (cell.index + direction + count) % count
        return MazeCell(ring: cell.ring, index: index)
    }

}

private struct SeededRandomNumberGenerator: RandomNumberGenerator {
    private var state: UInt64

    init(seed: UInt64) {
        state = seed
    }

    mutating func next() -> UInt64 {
        state &+= 0x9E3779B97F4A7C15
        var value = state
        value = (value ^ (value >> 30)) &* 0xBF58476D1CE4E5B9
        value = (value ^ (value >> 27)) &* 0x94D049BB133111EB
        return value ^ (value >> 31)
    }

    mutating func nextInt(in range: ClosedRange<Int>) -> Int {
        let width = UInt64(range.upperBound - range.lowerBound + 1)
        return range.lowerBound + Int(next() % width)
    }
}
