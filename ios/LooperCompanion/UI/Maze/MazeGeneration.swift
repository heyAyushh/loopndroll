import CoreGraphics

enum MazeKind {
    case circular
    case rectangular

    static func random(using random: inout SeededRandomNumberGenerator) -> MazeKind {
        random.nextInt(in: 0...99) < 58 ? .rectangular : .circular
    }
}

struct MazeCell: Hashable {
    let ring: Int
    let index: Int
}

struct GeneratedMaze {
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

struct MazeConnection: Hashable {
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

enum MazeGenerator {
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

struct RectCell: Hashable {
    let row: Int
    let column: Int
}

struct RectangularMaze {
    let columns: Int
    let rows: Int
    let start: RectCell
    let goal: RectCell
    let connections: Set<RectConnection>
    let solutionPath: [RectCell]

    var hasSolution: Bool {
        solutionPath.first == start && solutionPath.last == goal
    }

    var cells: [RectCell] {
        (0..<rows).flatMap { row in
            (0..<columns).map { column in
                RectCell(row: row, column: column)
            }
        }
    }

    func contains(_ cell: RectCell) -> Bool {
        cell.row >= 0 && cell.row < rows && cell.column >= 0 && cell.column < columns
    }

    func hasConnection(between first: RectCell, and second: RectCell) -> Bool {
        connections.contains(RectConnection(first, second))
    }
}

struct RectConnection: Hashable {
    let first: RectCell
    let second: RectCell

    init(_ first: RectCell, _ second: RectCell) {
        if first.row < second.row || first.row == second.row && first.column <= second.column {
            self.first = first
            self.second = second
        } else {
            self.first = second
            self.second = first
        }
    }
}

enum RectangularMazeGenerator {
    static func generate(columns: Int, rows: Int, seed: UInt64) -> RectangularMaze {
        var random = SeededRandomNumberGenerator(seed: seed)
        let start = RectCell(row: 0, column: 0)
        let goal = RectCell(row: rows - 1, column: columns - 1)
        let topology = RectangularMaze(
            columns: columns,
            rows: rows,
            start: start,
            goal: goal,
            connections: [],
            solutionPath: []
        )
        let connections = carvePerfectMaze(from: start, in: topology, using: &random)
        let solutionPath = path(from: start, to: goal, connections: connections, in: topology)

        return RectangularMaze(
            columns: columns,
            rows: rows,
            start: start,
            goal: goal,
            connections: connections,
            solutionPath: solutionPath
        )
    }

    private static func carvePerfectMaze(
        from start: RectCell,
        in maze: RectangularMaze,
        using random: inout SeededRandomNumberGenerator
    ) -> Set<RectConnection> {
        var visited: Set<RectCell> = [start]
        var active = [start]
        var connections: Set<RectConnection> = []
        let cellTotal = maze.columns * maze.rows

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

            connections.insert(RectConnection(current, next))
            visited.insert(next)
            active.append(next)
        }

        return connections
    }

    private static func activeCellIndex(
        from active: [RectCell],
        in maze: RectangularMaze,
        visited: Set<RectCell>,
        using random: inout SeededRandomNumberGenerator
    ) -> Int? {
        let candidates = active.indices.filter {
            !unvisitedNeighbors(from: active[$0], in: maze, visited: visited).isEmpty
        }

        guard !candidates.isEmpty else {
            return nil
        }

        if random.nextInt(in: 0...99) < 70 {
            return candidates.last
        }

        return candidates.randomElement(using: &random)
    }

    private static func chooseNeighbor(
        from neighbors: [RectCell],
        current: RectCell,
        using random: inout SeededRandomNumberGenerator
    ) -> RectCell? {
        guard !neighbors.isEmpty else {
            return nil
        }

        let weighted = neighbors.flatMap { neighbor -> [RectCell] in
            let towardGoal =
                (neighbor.row > current.row ? 2 : 0)
                + (neighbor.column > current.column ? 2 : 0)
            return Array(repeating: neighbor, count: 1 + towardGoal)
        }

        return weighted.randomElement(using: &random)
    }

    private static func path(
        from start: RectCell,
        to goal: RectCell,
        connections: Set<RectConnection>,
        in maze: RectangularMaze
    ) -> [RectCell] {
        var frontier = [start]
        var cameFrom: [RectCell: RectCell] = [:]
        var visited: Set<RectCell> = [start]

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
        of cell: RectCell,
        connections: Set<RectConnection>,
        in maze: RectangularMaze
    ) -> [RectCell] {
        neighbors(from: cell, in: maze).filter {
            connections.contains(RectConnection(cell, $0))
        }
    }

    private static func unvisitedNeighbors(
        from cell: RectCell,
        in maze: RectangularMaze,
        visited: Set<RectCell>
    ) -> [RectCell] {
        neighbors(from: cell, in: maze).filter { !visited.contains($0) }
    }

    private static func neighbors(from cell: RectCell, in maze: RectangularMaze) -> [RectCell] {
        [
            RectCell(row: cell.row + 1, column: cell.column),
            RectCell(row: cell.row, column: cell.column + 1),
            RectCell(row: cell.row - 1, column: cell.column),
            RectCell(row: cell.row, column: cell.column - 1),
        ].filter(maze.contains)
    }
}

struct SeededRandomNumberGenerator: RandomNumberGenerator {
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
