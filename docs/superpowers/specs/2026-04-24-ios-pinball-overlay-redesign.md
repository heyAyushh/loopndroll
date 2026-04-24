# iOS Pinball Overlay Redesign

Date: 2026-04-24
Status: Proposed
Owner: Codex

## Summary

Rebuild the experimental iOS physics layer as a small, explicit system that sits
on top of the current SwiftUI app instead of trying to infer physics from the
rendered view hierarchy.

The redesigned system treats the app as two cooperating layers:

1. SwiftUI owns application layout, navigation, and component state.
2. A dedicated overlay owns the frame loop, ball simulation, and rendering.

The overlay only collides with surfaces that are explicitly registered as live
physics boundaries. No generic UIKit scraping, stale snapshots, or fallback
rectangles are allowed as the source of truth.

## Problem Statement

The previous direction was wrong at the root:

- it tried to keep stock system chrome and derive exact collision geometry after
  render
- it mixed geometry discovery, collision rules, motion input, and rendering into
  one unstable architecture
- it pushed too much frame-by-frame work back through SwiftUI
- it relied on heuristics for surfaces that need deterministic geometry

This caused three persistent failures:

1. visible UI and collidable UI drifted apart
2. transitions created stale or guessed boundaries
3. performance and correctness both degraded as complexity increased

The redesign fixes this by making the physics layer explicit, bounded, and
separate from SwiftUI invalidation.

## Goals

- Make the whole app capable of hosting one experimental ball overlay.
- Keep frame-by-frame simulation out of SwiftUI view recomputation.
- Make collisions depend only on live, explicit geometry.
- Support device motion, haptics, and audio without broad app invalidation.
- Keep the architecture small enough to reason about and extend.

## Non-Goals

- Generic full-scene game engine integration.
- Reverse-engineering arbitrary stock UIKit internals for exact per-control
  collision.
- Visual redesign of the whole app in this phase.
- Multi-ball, level logic, score systems, or game rules.

## Design Constraints

- Use public Apple APIs only.
- Keep the production companion app architecture separate from the experimental
  overlay logic.
- If a boundary-critical system control cannot expose stable exact geometry via
  public APIs, replace that control with an owned lookalike instead of
  guessing.
- SwiftUI remains the owner of app UI state and layout.
- The physics overlay remains app-local and does not become the app’s main
  rendering model.

## Chosen Architecture

### 1. Surface Registry

Add one small registry that represents the current live collision world.

Each surface contains:

- stable identifier
- semantic role
- exact shape description
- current geometry in overlay coordinates
- physical material
- activity state

The registry only changes when a component’s layout or semantic state changes.
It does not tick every frame.

### 2. Explicit Surface Publishing

App-owned views publish surfaces directly.

Examples:

- bottom bar buttons
- owned search affordance
- custom cards
- custom orb controls

Publishing happens through a small SwiftUI-facing registration API. The API is
composable so new components can opt in without coupling themselves to the
physics engine.

### 3. Narrow Native Adapters

System-owned geometry may use dedicated adapters only where Apple exposes stable
public hooks.

Allowed examples:

- keyboard boundary
- sheet container frame
- coarse tab bar frame when exact item geometry is not required

Disallowed:

- generic subview scraping
- heuristic class-name matching
- accessibility-as-collision-truth

Accessibility may be used only as a debugging or semantic signal, not as the
authoritative geometry source.

### 4. UIKit-Backed Overlay

Render and simulate the ball inside a dedicated overlay view hosted from
SwiftUI through `UIViewRepresentable`.

This overlay owns:

- a `CADisplayLink`
- the mutable ball state
- collision resolution
- direct layer-backed drawing or animation

SwiftUI owns the overlay’s placement, not its per-frame internals.

### 5. Small Custom Physics Engine

Use a custom 2D solver instead of a heavyweight general engine.

Reasons:

- one ball is the core use case
- collisions are mostly circle-vs-curve or circle-vs-rounded-shape
- exactness and debuggability matter more than engine features
- the expensive problem here is surface truth, not rigid-body breadth

The engine will support:

- one moving ball
- deterministic time step
- device-orientation-aware gravity
- optional gyro impulse
- exact current surface collisions
- material-specific restitution and friction

### 6. Dedicated Motion Service

Motion input is isolated in one service.

It owns:

- gravity vector
- orientation transform into screen space
- optional gyro impulse contribution

The physics engine reads already-normalized motion state. SwiftUI views do not
own motion math.

### 7. Dedicated Feedback Service

Haptics and audio live behind one policy service.

It decides:

- which collisions are meaningful enough to fire
- which haptic class to use
- which sound to play
- cooldown and rate limiting

This avoids scattering feedback rules across UI code and keeps collision output
testable.

## Data Flow

1. SwiftUI lays out the current screen.
2. Registered components publish or update live surfaces in the registry.
3. Native adapters publish only the supported system surfaces.
4. The overlay receives the current surface snapshot.
5. The display link ticks.
6. The motion service provides current input.
7. The physics engine advances the ball against the current surface snapshot.
8. The feedback service reacts to meaningful impacts.
9. The overlay redraws the ball.

There is no frame-by-frame SwiftUI state mutation in this loop.

## Registration Model

Provide one app-facing registration API with a small surface model.

Required surface shapes in phase one:

- circle
- capsule
- rounded rectangle
- polygon path

Required material types in phase one:

- rigid glass
- soft glass
- metal
- muted boundary

Every registered surface defaults to active collision unless explicitly disabled.

## Rendering Strategy

The overlay renders above the app UI so the ball stays visible.

Physical interaction remains tied to the registered geometry of the components
below it. The first implementation keeps component visuals unchanged and limits
feedback to the ball plus haptics and audio.

## Performance Strategy

The main performance rule is separation of responsibilities:

- SwiftUI updates on layout and state changes
- the overlay updates on display refresh

No broad `@Observable` model or view state may be used as the physics clock.
No filtering, sorting, or surface recomputation may happen inside a frame-tick
SwiftUI body path.

Specific rules:

- keep the surface registry immutable per frame and replace snapshots only on
  change
- keep the physics engine state local to the overlay
- avoid re-rendering the whole overlay subtree when only the ball position
  changes
- prefer direct layer updates or lightweight custom drawing over rebuilding
  SwiftUI view hierarchies

## Simplification Decisions

To keep the redesign small and correct:

- remove any previous generic surface-inspection architecture
- do not build a reusable surface-graph framework first
- keep the first implementation app-local and explicit
- prove the architecture with the bottom bar and search affordance first
- only extract shared infrastructure after the minimal design works

This is intentionally narrower than the previous direction because correctness
matters more than abstraction.

## Integration Plan

### Phase 1: New Overlay Core

- add a dedicated overlay host
- add the custom ball engine
- add motion input service
- add feedback service
- add a local surface registry

### Phase 2: Explicit Surface Ownership

- register bottom bar controls explicitly
- register search affordance explicitly
- remove any guessed or generic surface capture still affecting these controls

### Phase 3: Device Boundary Correctness

- use current device safe-area and screen bounds correctly
- keep bezel treatment explicit and minimal
- avoid invisible inset clipping boxes

### Phase 4: Extension Path

After the core is correct, add additional app-owned surfaces one class at a
time:

- cards
- rows
- sheet-owned controls

Scanner overlays are explicitly out of scope for the first implementation pass.

## Testing Plan

### Unit-Level

- surface registry add, update, remove
- deterministic physics step behavior
- collision resolution against each supported shape
- orientation transform correctness
- feedback gating and cooldown logic

### Integration-Level

- ball collides with bottom bar controls in their current visible positions
- search collapsed and expanded states publish the correct live boundary
- no stale boundary remains after transitions
- rotating the phone changes the effective gravity direction correctly

### Device Validation

- verify on a physical iPhone
- confirm the ball remains visible during interaction
- confirm no frame hitching from broad SwiftUI invalidation
- confirm haptics and audio fire only on meaningful impacts

## Finish Criteria

The redesign is complete only when all of the following are true:

- the overlay runs without frame-by-frame SwiftUI recomputation
- the ball collides only with currently registered live surfaces
- bottom bar and search interactions use explicit current geometry rather than
  guessed or stale geometry
- no generic surface scraping remains in the critical path
- the code is smaller and easier to reason about than the previous direction

## Decision Summary

The best path is:

- explicit surfaces
- custom lightweight physics
- UIKit-backed overlay
- narrow native adapters only where public APIs are good enough
- no more heuristic geometry recovery as the core architecture
