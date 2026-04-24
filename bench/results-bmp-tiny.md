# rsteg bench report — bmp-tiny

Platform: macos/aarch64 ncpu=28. `steghide` is a local native-arm64 build of
0.6.0. Only rsteg p50 rows are full p50/p95/MAD; steghide rows report p50
only from the same native run.

## bmp-tiny — embed 16 B

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 2.40          | 2.85     | 0.16     | 0.00    | 0             | 0      |
| steghide | 6.42          |    —     |    —     | 0.00    | 0             | 0      |

## bmp-tiny — embed 1 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 1.61          | 4.36     | 0.17     | 0.00    | 0             | 0      |
| steghide | 20.58         |    —     |    —     | 0.00    | 0             | 0      |

## bmp-tiny — embed 4 KB

| Tool  | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|-------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.62          | 4.59     | 0.19     | 0.00    | 0             | 0      |

`steghide` rejects 4 KB payloads against this cover (capacity exceeded).

## bmp-tiny — extract 16 B

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 1.73          | 2.83     | 0.13     | 0.00    | 0             | 0      |
| steghide | 5.48          |    —     |    —     | 0.00    | 0             | 0      |

## bmp-tiny — extract 1 KB

| Tool     | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|----------|---------------|----------|----------|---------|---------------|--------|
| rsteg    | 1.46          | 2.53     | 0.06     | 0.00    | 0             | 0      |
| steghide | 6.48          |    —     |    —     | 0.00    | 0             | 0      |

## bmp-tiny — extract 4 KB

| Tool  | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|-------|---------------|----------|----------|---------|---------------|--------|
| rsteg | 2.21          | 2.50     | 0.09     | 0.00    | 0             | 0      |
