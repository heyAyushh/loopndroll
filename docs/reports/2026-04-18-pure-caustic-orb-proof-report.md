# Pure Caustic Orb Proof Report

## Parameters
- orientation atoms: 3
- payload atoms: 136
- sphere samples: 384
- image sample side: 18
- yaw steps: 12
- monte carlo samples: 64

## Structural Certificate
- max orientation/payload inner product: 2.500021e-13
- frame lower bound: 1.000000
- frame upper bound: 1.000000
- coefficient margin: 0.220000
- image operator lower bound: 0.267827

## Decoder Metrics
- image margin: 0.023348
- shortlist true hit rate: 1.000000
- stage two success rate: 1.000000
- residual variance: 0.000004
- block error upper bound: 2.531950e-8
- tested samples: 64

## Assumptions
- The proof is exact for the encoder and sampled-basis construction.
- The optical shell is represented by a fixed three-path analytic transport model.
- Camera variability is factorized into yaw, affine exposure, orientation leakage, and additive Gaussian-like luma noise.
- The error bound applies only inside this explicit model family, not to arbitrary real-world captures.
