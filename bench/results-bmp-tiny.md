# rsteg bench report

Platform: macos/aarch64 ncpu=28

## bmp-tiny — embed 16 B

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.42 | 2.85 | 0.16 | 0.00 | 0 | 0 |
| steghide | 2177.83 | 4577.23 | 581.66 | 0.00 | 0 | 0 |

## bmp-tiny — embed 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.60 | 4.36 | 0.17 | 0.00 | 0 | 0 |
| steghide | 2373.15 | 5684.58 | 1087.43 | 0.00 | 0 | 0 |

## bmp-tiny — embed 4 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.62 | 4.59 | 0.19 | 0.00 | 0 | 0 |

## bmp-tiny — extract 16 B

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.27 | 2.83 | 0.13 | 0.00 | 0 | 0 |
| steghide | 3305.67 | 6039.59 | 652.10 | 0.00 | 0 | 0 |

## bmp-tiny — extract 1 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.35 | 2.53 | 0.06 | 0.00 | 0 | 0 |
| steghide | 2352.07 | 4037.07 | 438.01 | 0.00 | 0 | 0 |

## bmp-tiny — extract 4 KB

| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.21 | 2.50 | 0.09 | 0.00 | 0 | 0 |

