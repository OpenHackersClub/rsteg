# rsteg bench report — bmp-small

Platform: macos/aarch64 ncpu=28. `steghide` is a local native-arm64 build of
0.6.0; see [`README.md`](README.md#native-steghide-build) for the build
steps. rsteg rows are full p50/p95/MAD; steghide rows report p50 only
from the same native run.

## bmp-small — embed 1 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 3.77          | 4.48     | 0.13     | 0.00    | 0             | 0      |
| steghide | 41.15         |    —     |    —     | 0.00    | 0             | 0      |

## bmp-small — embed 10 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 3.51          | 4.18     | 0.11     | 0.00    | 0             | 0      |
| steghide | 440.09        |    —     |    —     | 0.00    | 0             | 0      |

## bmp-small — embed 65 KB

| Tool  | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|-------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 4.18          | 4.54     | 0.20     | 0.00    | 0             | 0      |

## bmp-small — extract 1 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 2.83          | 3.33     | 0.10     | 0.00    | 0             | 0      |
| steghide | 17.14         |    —     |    —     | 0.00    | 0             | 0      |

## bmp-small — extract 10 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 2.74          | 3.23     | 0.14     | 0.00    | 0             | 0      |
| steghide | 28.14         |    —     |    —     | 0.00    | 0             | 0      |

## bmp-small — extract 65 KB

| Tool  | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|-------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 3.18          | 3.74     | 0.10     | 0.00    | 0             | 0      |
