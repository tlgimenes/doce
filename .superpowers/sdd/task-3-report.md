# Task 3 Report: Sampling alignment with Qwen 2507 guidance

## Status
**COMPLETE**

## Commit Hash
`f961762`

## Verification Summary
Sampler chain successfully aligned to Qwen3-*-2507 recommended parameters: replaced repeat-penalty (64, 1.1, 0.0, 0.0) with presence-penalty model (64, 1.0, 0.0, 1.0); updated top-k from 40→20, top-p from 0.9→0.8; added min_p(0.0, 1).

## Implementation Details

### Step 1: Verified min_p signature
Confirmed `LlamaSampler::min_p(p: f32, min_keep: usize)` exists in llama-cpp-2-0.1.150 at line 286 of sampling.rs.

### Step 2: Replaced sampler chain
Updated `/Users/gimenes/code/doce/src-tauri/src/inference/mod.rs` lines 462-468 with Qwen-recommended parameters plus motivating comment explaining the presence-penalty shift for grammar-safe repetition control.

**Old chain:**
```rust
chain.extend([
    LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
    LlamaSampler::top_k(40),
    LlamaSampler::top_p(0.9, 1),
    LlamaSampler::temp(0.7),
    LlamaSampler::dist(seed),
]);
```

**New chain:**
```rust
// Qwen3-*-2507's own recommended sampling (model card): temp 0.7,
// top-p 0.8, top-k 20, min-p 0 — with presence-penalty for
// repetition control instead of repeat-penalty (repeat-penalty
// taxes the tokens JSON repeats BY DESIGN — braces, quotes, key
// names — and inside an active grammar it can only distort
// argument content).
chain.extend([
    LlamaSampler::penalties(64, 1.0, 0.0, 1.0),
    LlamaSampler::top_k(20),
    LlamaSampler::top_p(0.8, 1),
    LlamaSampler::min_p(0.0, 1),
    LlamaSampler::temp(0.7),
    LlamaSampler::dist(seed),
]);
```

### Step 3: Verification
- `cargo test --lib`: **PASS** (244 passed; 0 failed; 2 ignored)
- `cargo clippy --lib`: **CLEAN** (no warnings)

### Step 4: Commit
Git commit f961762 created with message and co-author attribution as specified.

## Concerns
None. The benchmark gate was explicitly not run per controller instructions; the changes are minimal and well-documented.
