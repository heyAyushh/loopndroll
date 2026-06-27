# LooperClientCore

Swift package wrapper for the Rust `looper-client-core` UniFFI boundary.

Rebuild the generated Swift bindings and XCFramework from the repository root:

```bash
bash scripts/build-looper-client-core-package.sh
```

The generated Swift sources live under `Sources/LooperClientCore/Generated`.
The binary target lives at `Frameworks/LooperClientCoreFFI.xcframework`.

This package should stay a view wrapper over the Rust client core. Do not add
HTTP command routes, event-stream clients, SQLite ownership, or session-control truth in
Swift.
