# rsteg bench report

Platform: macos/aarch64 ncpu=28

## wav-short — embed 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.37 | 2.57 | 0.08 | 0.00 | 0 | 0 |
| steghide | 372.76 | 567.43 | 11.92 | 0.00 | 0 | 0 |
| stegano | 69.04 | 71.75 | 0.25 | 0.00 | 0 | 0 |

## wav-short — embed 20 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 3.48 | 4.73 | 0.13 | 0.00 | 0 | 0 |
| steghide | 2766.41 | 3155.41 | 102.56 | 0.00 | 0 | 0 |
| stegano | 70.75 | 82.30 | 0.35 | 0.00 | 0 | 0 |

## wav-short — extract 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 1.86 | 2.12 | 0.03 | 0.00 | 0 | 0 |
| steghide | 659.73 | 958.73 | 79.85 | 0.00 | 0 | 0 |
| stegano | 68.18 | 68.95 | 0.24 | 0.00 | 0 | 0 |

## wav-short — extract 20 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.90 | 3.06 | 0.13 | 0.00 | 0 | 0 |
| steghide | 1288.38 | 1993.63 | 230.64 | 0.00 | 0 | 0 |
| stegano | 68.43 | 69.33 | 0.26 | 0.00 | 0 | 0 |

