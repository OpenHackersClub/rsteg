# rsteg bench report — wav-short

Platform: macos/aarch64 ncpu=28.

Native arm64 `steghide 0.6.0` reliably SIGSEGVs when embedding any payload
≥ 1 KB into this cover, and stack-limit tuning doesn't help. The valid
comparison on this case is rsteg vs `stegano-cli`.

## wav-short — embed 1 KB

| Tool    | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|---------|---------------|----------|----------|---------|---------------|--------|
| rsteg   | 2.37          | 2.57     | 0.08     | 0.00    | 0             | 0      |
| stegano | 69.04         | 71.75    | 0.25     | 0.00    | 0             | 0      |

## wav-short — embed 20 KB

| Tool    | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|---------|---------------|----------|----------|---------|---------------|--------|
| rsteg   | 3.48          | 4.73     | 0.13     | 0.00    | 0             | 0      |
| stegano | 70.75         | 82.30    | 0.35     | 0.00    | 0             | 0      |

## wav-short — extract 1 KB

| Tool    | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|---------|---------------|----------|----------|---------|---------------|--------|
| rsteg   | 1.86          | 2.12     | 0.03     | 0.00    | 0             | 0      |
| stegano | 68.18         | 68.95    | 0.24     | 0.00    | 0             | 0      |

## wav-short — extract 20 KB

| Tool    | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |
|---------|---------------|----------|----------|---------|---------------|--------|
| rsteg   | 2.90          | 3.06     | 0.13     | 0.00    | 0             | 0      |
| stegano | 68.43         | 69.33    | 0.26     | 0.00    | 0             | 0      |
