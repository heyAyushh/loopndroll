# Looper iOS Companion V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a real native iPhone app in this repo that shows the approved Looper companion experience, uses haptics, and can run against a local Bun dev API with Looper-shaped data.

**Architecture:** Add a new `ios/` XcodeGen-based SwiftUI app with a small app shell, typed models, a live/mock data service, and haptic helpers. Add a Bun HTTP dev API on the Mac side that exposes a narrow mobile-facing snapshot and mutation surface, so the simulator can show realistic data now and later evolve into the Tailscale control boundary.

**Tech Stack:** SwiftUI, Observation, URLSession, XcodeGen, Bun HTTP server, TypeScript strict mode, existing Looper Bun actions.

---

### Task 1: Create the implementation branch and iOS project scaffold

**Files:**
- Create: `ios/project.yml`
- Create: `ios/LooperCompanion/LooperCompanionApp.swift`
- Create: `ios/LooperCompanion/Assets.xcassets/Contents.json`
- Create: `ios/LooperCompanion/Preview Content/Preview Assets.xcassets/Contents.json`
- Modify: `.gitignore`

- [ ] **Step 1: Move implementation off `main`**

Run: `git switch -c cx/ios-companion-app`
Expected: shell prints `Switched to a new branch 'cx/ios-companion-app'`

- [ ] **Step 2: Add the iOS project scaffold files**

Create `ios/project.yml` with an iOS application target named `LooperCompanion`, deployment target `18.0`, bundle id `com.looper.companion`, asset catalogs, and source roots under `ios/LooperCompanion`.

Create `ios/LooperCompanion/LooperCompanionApp.swift` with the minimal app entry:

```swift
import SwiftUI

@main
struct LooperCompanionApp: App {
    var body: some Scene {
        WindowGroup {
            Text("Looper Companion")
        }
    }
}
```

- [ ] **Step 3: Generate the Xcode project**

Run: `cd ios && xcodegen generate`
Expected: output includes `Generated project at`

- [ ] **Step 4: Build the empty app once**

Run: `xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build`
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 5: Commit the scaffold**

```bash
git add .gitignore ios
git commit -m "feat: scaffold iOS companion app" -m "- add an XcodeGen-based iPhone app target\n- generate the initial SwiftUI app shell\n- prepare the repo for companion app implementation"
```

### Task 2: Add typed companion models, mock data, and haptic support

**Files:**
- Create: `ios/LooperCompanion/App/CompanionAppModel.swift`
- Create: `ios/LooperCompanion/App/CompanionEnvironment.swift`
- Create: `ios/LooperCompanion/Models/CompanionModels.swift`
- Create: `ios/LooperCompanion/Support/Haptics.swift`
- Create: `ios/LooperCompanion/Support/PreviewFixtures.swift`

- [ ] **Step 1: Define the mobile-facing models**

Add `CompanionModels.swift` with focused structs:

```swift
import Foundation

struct MobileSnapshot: Decodable, Sendable {
    var host: HostSummary
    var globalSettings: GlobalSettings
    var sessions: [SessionSummary]
    var notifications: [NotificationDestination]
    var completionChecks: [CompletionCheckSummary]
}
```

Include `HostSummary`, `GlobalSettings`, `SessionSummary`, `SessionDetail`, `NotificationDestination`, `CompletionCheckSummary`, `QuickActionOption`, and enums for mode and connectivity state.

- [ ] **Step 2: Add preview and mock fixtures**

Create `PreviewFixtures.swift` with one realistic snapshot and one detailed session fixture that match the approved design:

```swift
enum PreviewFixtures {
    static let snapshot = MobileSnapshot(...)
    static let sessionDetail = SessionDetail(...)
}
```

- [ ] **Step 3: Add a haptic helper**

Create `Haptics.swift` with a small wrapper:

```swift
import SwiftUI
import UIKit

enum Haptics {
    static func success() { UINotificationFeedbackGenerator().notificationOccurred(.success) }
    static func warning() { UINotificationFeedbackGenerator().notificationOccurred(.warning) }
    static func error() { UINotificationFeedbackGenerator().notificationOccurred(.error) }
    static func impact() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
}
```

- [ ] **Step 4: Add the root app model**

Create `CompanionAppModel.swift` as an `@Observable` root-owned state container with:

```swift
@Observable
final class CompanionAppModel {
    var snapshot: MobileSnapshot?
    var selectedSessionID: String?
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
}
```

- [ ] **Step 5: Build to catch type errors**

Run: `xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build`
Expected: `** BUILD SUCCEEDED **`

### Task 3: Build the SwiftUI app shell and native screens

**Files:**
- Create: `ios/LooperCompanion/UI/Root/RootTabView.swift`
- Create: `ios/LooperCompanion/UI/Sessions/SessionsScreen.swift`
- Create: `ios/LooperCompanion/UI/Sessions/SessionRow.swift`
- Create: `ios/LooperCompanion/UI/Sessions/SessionDetailScreen.swift`
- Create: `ios/LooperCompanion/UI/Settings/SettingsScreen.swift`
- Create: `ios/LooperCompanion/UI/Common/StatusPill.swift`
- Modify: `ios/LooperCompanion/LooperCompanionApp.swift`

- [ ] **Step 1: Replace the placeholder app entry with an environment-backed shell**

Update `LooperCompanionApp.swift` so it creates a `CompanionAppModel`, injects a service, and renders `RootTabView`.

- [ ] **Step 2: Build the two-tab native shell**

Create `RootTabView.swift` with `TabView`, `NavigationStack`, and two tabs:

```swift
Tab("Sessions", systemImage: "message.badge.waveform") { SessionsScreen(model: model) }
Tab("Settings", systemImage: "gearshape") { SettingsScreen(model: model) }
```

- [ ] **Step 3: Build the sessions list and summary header**

Create `SessionsScreen.swift` with:
- a connection summary card
- a segmented control for active/archived
- a list of `SessionRow`
- `.task` loading

Use haptics when applying quick session actions.

- [ ] **Step 4: Build session detail**

Create `SessionDetailScreen.swift` with:
- title/ref/status
- assistant preview
- mode picker
- notifications section
- completion checks section
- archive/delete buttons

Use `confirmationDialog` for destructive actions and warning haptics on confirm.

- [ ] **Step 5: Build settings**

Create `SettingsScreen.swift` with sections for:
- continue prompt
- push actions
- paired Mac info
- notification destinations
- completion checks

- [ ] **Step 6: Build and preview the shell**

Run: `xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build`
Expected: `** BUILD SUCCEEDED **`

### Task 4: Add a live/mock service layer for the iPhone app

**Files:**
- Create: `ios/LooperCompanion/Services/CompanionService.swift`
- Create: `ios/LooperCompanion/Services/HTTPCompanionService.swift`
- Create: `ios/LooperCompanion/Services/MockCompanionService.swift`
- Modify: `ios/LooperCompanion/App/CompanionAppModel.swift`
- Modify: `ios/LooperCompanion/App/CompanionEnvironment.swift`

- [ ] **Step 1: Define the service protocol**

Create `CompanionService.swift`:

```swift
protocol CompanionService: Sendable {
    func loadSnapshot() async throws -> MobileSnapshot
    func loadSessionDetail(id: String) async throws -> SessionDetail
}
```

- [ ] **Step 2: Add the mock implementation**

Create `MockCompanionService.swift` that returns `PreviewFixtures.snapshot` and `PreviewFixtures.sessionDetail`.

- [ ] **Step 3: Add the HTTP implementation**

Create `HTTPCompanionService.swift` using `URLSession` and a base URL from environment/config:

```swift
struct HTTPCompanionService: CompanionService {
    let baseURL: URL
}
```

Decode JSON from:
- `GET /api/mobile/snapshot`
- `GET /api/mobile/sessions/:id`

- [ ] **Step 4: Wire the app model to the service**

Update `CompanionAppModel` with `loadSnapshot()` and `loadSessionDetail()` async methods and set clear connection states for loading, connected, and offline.

- [ ] **Step 5: Verify the app still builds**

Run: `xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build`
Expected: `** BUILD SUCCEEDED **`

### Task 5: Add the Bun mobile dev API

**Files:**
- Create: `src/shared/mobile-contract.ts`
- Create: `legacy/bun/mobile-dev-server.ts`
- Create: `legacy/bun/mobile-mappers.ts`
- Modify: `package.json`

- [ ] **Step 1: Define the shared mobile contract**

Create `src/shared/mobile-contract.ts` with the mobile-facing types mirrored from the Swift models:

```ts
export type MobileSnapshot = {
  host: HostSummary;
  globalSettings: GlobalSettings;
  sessions: SessionSummary[];
  notifications: MobileNotification[];
  completionChecks: MobileCompletionCheck[];
};
```

- [ ] **Step 2: Map existing Looper snapshot data into the mobile contract**

Create `mobile-mappers.ts` with pure mapping helpers:

```ts
export function mapLooperSnapshotToMobile(snapshot: LooperSnapshot): MobileSnapshot
export function mapLoopSessionToDetail(session: LoopSession): MobileSessionDetail
```

- [ ] **Step 3: Add the Bun HTTP dev server**

Create `mobile-dev-server.ts` with endpoints:
- `GET /api/mobile/snapshot`
- `GET /api/mobile/sessions/:id`

Use the existing `ensureLooperSetup()` and `getLooperSnapshot()` flow, then map to the mobile contract.

- [ ] **Step 4: Add a dev script**

Update `package.json` with:

```json
"dev:ios-api": "bun run legacy/bun/mobile-dev-server.ts"
```

- [ ] **Step 5: Verify the dev API responds**

Run: `bun run legacy/bun/mobile-dev-server.ts`

Then in a second shell run:

```bash
curl http://127.0.0.1:8787/api/mobile/snapshot
```

Expected: valid JSON containing `host`, `globalSettings`, and `sessions`

### Task 6: Connect the running iPhone app to the Bun dev API and verify in simulator

**Files:**
- Modify: `ios/project.yml`
- Modify: `ios/LooperCompanion/App/CompanionEnvironment.swift`
- Modify: `ios/LooperCompanion/Services/HTTPCompanionService.swift`

- [ ] **Step 1: Add a debug base URL config**

Set a debug environment value in `ios/project.yml`:

```yaml
settings:
  base:
    LOOPER_API_BASE_URL: http://127.0.0.1:8787
```

- [ ] **Step 2: Resolve the service from configuration**

Update `CompanionEnvironment.swift` so Debug builds prefer `HTTPCompanionService` and fall back to mock data if the base URL is missing.

- [ ] **Step 3: Boot the simulator**

Run: `xcrun simctl boot "iPhone 17 Pro"`
Expected: either no output or a message that it is already booted

- [ ] **Step 4: Start the Bun mobile dev API**

Run: `pnpm run dev:ios-api`
Expected: server prints that it is listening on `127.0.0.1:8787`

- [ ] **Step 5: Build and run the app in the simulator**

Run:

```bash
xcodebuild \
  -project ios/LooperCompanion.xcodeproj \
  -scheme LooperCompanion \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
  build
```

Then install and launch with `xcrun simctl install` and `xcrun simctl launch`.

Expected: app opens on the simulator and shows the Looper companion shell with sessions and settings.

- [ ] **Step 6: Capture verification artifacts**

Run:

```bash
xcrun simctl io booted screenshot artifacts/ios-companion-home.png
```

Expected: screenshot file exists and clearly shows the app UI.

### Task 7: Run repo checks and finalize the slice

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the iPhone app entry point**

Add a short `README.md` section describing:
- where the iOS app lives
- how to generate the Xcode project
- how to run the Bun dev API
- how to build/run the simulator app

- [ ] **Step 2: Run repo checks**

Run: `pnpm check`
Expected: lint, format check, and typecheck pass

- [ ] **Step 3: Build the iPhone app one last time**

Run: `xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build`
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 4: Commit the feature**

```bash
git add README.md package.json src/shared/mobile-contract.ts legacy/bun/mobile-dev-server.ts legacy/bun/mobile-mappers.ts ios
git commit -m "feat: add iPhone companion app shell" -m "- add a native SwiftUI Looper companion app\n- add a Bun mobile dev API for simulator integration\n- add haptics, session screens, and settings scaffolding"
```
