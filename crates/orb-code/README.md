# orb-code

Rust library and CLI for deterministic orb images with spherical harmonic
caustics and an orb-native ring payload carrying `orb_id`.

## CLI

Print the deterministic id without generating a PNG:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-code -- id --data "https://example.com"
```

Generate from an explicit id:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-code -- generate --id demo_orb --out demo_orb.png
```

Generate from arbitrary data:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-code -- generate --data "https://example.com" --out example_orb.png
```

Scan an orb image:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-code -- scan example_orb.png
```

Verify that the visible orb art matches the embedded payload:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-code -- verify example_orb.png
```

Generate the analytical pure-caustic proof report:

```bash
cargo run --manifest-path crates/orb-code/Cargo.toml --bin orb-caustic-proof
```

## Library

The public API is built for reuse:

- `generate_orb_image`
- `scan_orb_image`
- `verify_orb_image`
- `derive_orb_id`

The image format is hybrid:

- deterministic orb art generated from `orb_id`
- visible orb-native ring payload carrying only `orb_id`
- optional art verification against the decoded id

The crate also contains a parallel analytical proof subsystem under
`src/caustic`. It does not claim unconstrained real-world QR robustness. It
proves the exact encoder layer and emits a sampled numerical certificate for the
fixed-shell camera model used by the proof report.

## iOS package

Build the attachable Swift package and xcframework with:

```bash
bash scripts/build-orb-code-ios-package.sh
```

The resulting package lives at `ios/OrbCodeKit` and can be added to an iOS app as a local Swift package.
