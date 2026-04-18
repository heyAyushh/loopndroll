# Loopndroll Orb Code Design

Date: 2026-04-18
Status: Approved direction, pending written spec review

## Summary

Replace the current QR-backed orb format with an orb-only marker.

The new image remains a single spherical orb, but the machine-readable
information moves into a circular signal ring integrated near the orb
perimeter. The glassy center stays branded and expressive. The scanner
decodes the circular structure directly from a camera image instead of
reading a separate QR code.

This design follows the main lesson from the fiducial-marker literature:
camera robustness comes from explicit, repeatable geometry, while the
decorative or branded appearance should stay secondary.

## Goals

- Ship a single-image orb marker with no QR code.
- Keep deterministic ID generation in Rust.
- Make iOS camera capture realistic by encoding the ID in explicit
  circular structure rather than soft appearance alone.
- Preserve the orb as the product surface instead of switching to a
  square or utilitarian tag.
- Keep the public Rust and Swift APIs stable where practical.

## Non-goals

- End-to-end learned detection in v1.
- Hidden or steganographic payload encoding.
- Arbitrary large payloads embedded directly in the marker.
- Perfect recognition under severe glare, heavy occlusion, or extreme
  motion blur.
- Reproducing the HTML reference literally.

## Research Conclusion

The reviewed literature points in a consistent direction:

- AprilTag and AprilTag 2 favor simple, high-contrast geometry for low
  resolution and hard viewing conditions.
- CCTag, STag, and WhyCode show that circles and concentric structure
  localize more repeatably than soft texture.
- ARTTag shows that aesthetic markers can still keep explicit geometric
  detection anchors.
- CylinderTag shows that once a marker rides on a curved form, the
  encoding should respect that geometry.
- Learnable Visual Markers, DeepTag, E2ETag, and DeepFormableTag show
  that stylized appearance-first markers are possible, but only with a
  trained detector and a larger data and training pipeline.

Inference from those sources: a pure caustic or reflective orb should
not be the primary carrier of the ID in v1. It can be a deterministic
verification layer, but the bits should live in a stable circular code.

## Recommended Approach

Use a hybrid orb-only format:

- a glossy branded core for the orb identity
- a matte circular signal ring for the machine-readable ID
- explicit guard and orientation geometry for camera localization
- a deterministic secondary fingerprint from the core for verification

This keeps the marker as an orb while avoiding the main weakness of the
current prototype, where the scanner depends on a separate QR and the
orb itself is not truly decodable.

## Marker Format

The output is a square PNG containing one orb centered on a quiet
background.

The orb is divided into five radial zones:

1. `background`
2. `outer silhouette ring`
3. `outer guard ring`
4. `payload annulus`
5. `glass core`

The glass core is visual and deterministic. The payload annulus is the
actual code.

### Visual Rules

- The guard ring and payload annulus must be matte, crisp, and high
  contrast.
- The glass core may contain blur, caustics, soft highlights, and
  spherical harmonic structure.
- No payload information should depend on soft highlights, glow, or
  reflection.
- The orb should read as one coherent object rather than a symbol pasted
  on top of another image.

## Payload Layout

Version 1 uses a circular binary code laid out in polar coordinates.

### Angular layout

- `56` equal angular sectors around the orb
- sectors indexed clockwise after normalization

### Radial layout inside the payload annulus

- `3` payload tracks
- each sector therefore carries `3` binary cells

This yields `168` raw cells.

### Reserved structure

- `4` sectors are reserved for orientation and sync
- sync cells are fixed and asymmetric so rotation can be solved without
  ambiguity
- remaining `52` sectors carry payload

`52 sectors * 3 tracks = 156 payload cells`

### Bit allocation

- `4` bits: payload version
- `8` bits: format flags and reserved future use
- `128` bits: orb payload
- `16` bits: CRC-16 over version, flags, and payload

Total: `156` bits

### Orb payload

The encoded payload is the raw 128-bit orb seed. The textual `orb_id`
shown by the CLI and SDK remains a formatted representation of that same
seed.

This keeps the current deterministic identity model while avoiding the
need to encode the literal ASCII string in the ring.

## ID Model

The ID model stays deterministic:

- `derive_orb_id(data)` hashes the input data
- the library stores the first 128 bits as the orb seed
- the public `orb_id` remains a stable textual form of that seed

The renderer and decoder both use the 128-bit seed as the canonical
identity.

## Rendering Design

### Core

The core should look better than the current washed-out orb:

- darker midtones
- smaller, sharper specular highlights
- less front-facing haze
- stronger edge definition
- more obvious internal structure with cleaner caustic bands

The current output looks like a pale smudge because the brightest zones
occupy too much of the frontal area and the core lacks enough local
contrast. The new renderer should bias toward deeper glass and tighter
energy distribution.

### Signal ring

The signal ring should look intentional and on-brand:

- narrow enough to preserve the orb aesthetic
- thick enough to survive phone capture
- monochrome in v1 for simpler thresholding
- separated from the core by a thin neutral gap

The ring can use slightly warm or cool neutrals later, but v1 should
stay effectively binary when converted to grayscale.

## Decoder Pipeline

The scanner should stop looking for QR grids entirely.

### Detection

1. Convert the image to grayscale.
2. Detect candidate circular or elliptical orb boundaries.
3. Refine the best candidate with edge-based ellipse fitting.
4. Estimate the annulus bounds from the expected radial proportions.

### Normalization

5. Warp the annulus into a polar unwrap image.
6. Sample the three payload tracks at fixed radii.
7. Compute adaptive thresholds using the guard ring and local sector
   statistics.

### Decoding

8. Search all 56 rotations for the sync pattern.
9. Read the 156 payload bits.
10. Validate version and CRC.
11. Recover the 128-bit seed and reconstruct the textual `orb_id`.

### Verification

12. Optionally re-render the expected core from the decoded seed.
13. Compare a compact fingerprint from the captured core against the
    expected core as a secondary integrity check.

The core fingerprint is not the primary decoder. It is only a tie-break
or tamper check.

## iOS SDK Direction

The Swift package surface should stay simple:

- generate orb PNG from `orb_id`
- generate orb PNG from arbitrary data
- scan `orb_id` from image bytes
- verify an orb image against its decoded ID

Internally, the Swift wrapper can keep calling the Rust FFI. The API
should not expose QR-specific concepts because the format is now orb-only.

## Error Handling

The implementation should distinguish:

- unreadable image
- orb not found
- annulus found but sync missing
- sync found but payload invalid
- CRC mismatch
- unsupported payload version
- payload decoded but core verification failed

These should stay typed Rust errors and map cleanly through FFI.

## Compatibility and Migration

The old QR-backed format should be considered deprecated.

Implementation should:

- remove QR generation from the renderer
- remove QR scanning from the decode path
- keep the top-level library function names when practical
- regenerate the iOS static library and xcframework against the new
  format

Backward compatibility with existing QR orbs is not required unless
explicitly requested later.

## Testing

The work is done when it proves all of the following:

- the same input produces the same seed, `orb_id`, and orb image
- generated orb images decode back to the same `orb_id`
- decoding works after representative perspective warp, blur, brightness
  shift, and JPEG recompression in tests
- decode failures are categorized correctly
- core verification can detect a mismatched center when the signal ring
  decodes but the visual core has been altered

Tests should include:

- unit tests for bit packing and CRC
- unit tests for sync detection and rotation recovery
- golden tests for render determinism
- round-trip tests for generate/scan/verify
- perturbation tests using synthetic image transforms

## First Implementation Slice

Implement the new system in this order:

1. add seed-to-ring bit packing and CRC
2. replace QR rendering with guard rings plus payload annulus
3. replace QR scanning with orb detection, annulus unwrap, and bit decode
4. keep or improve the existing core fingerprint verification
5. update the CLI and Swift package tests

This keeps the public product shape intact while swapping out the
underlying transport mechanism in one coherent pass.
