# rsteg bench report

Platform: macos/aarch64 ncpu=28

## png-synth-small — embed 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 6.09 | 6.44 | 0.11 | 0.00 | 0 | 0 |
| stegano | 66.69 | 67.79 | 0.46 | 0.00 | 0 | 65526 |

## png-synth-small — embed 10 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 6.16 | 6.38 | 0.11 | 0.00 | 0 | 0 |
| stegano | 67.97 | 69.90 | 0.37 | 0.00 | 0 | 65526 |

## png-synth-small — extract 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.60 | 3.34 | 0.09 | 0.00 | 0 | 0 |
| stegano | 66.67 | 67.79 | 0.24 | 0.00 | 0 | 0 |

## png-synth-small — extract 10 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.60 | 2.92 | 0.10 | 0.00 | 0 | 0 |
| stegano | 66.39 | 67.65 | 0.36 | 0.00 | 0 | 0 |

