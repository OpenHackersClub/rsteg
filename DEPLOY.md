# Deploying the demo site

The `rsteg` demo site is a static export of `crates/rsteg-web/src/index.html`,
checked in at `public/index.html`. It's published to **Cloudflare Pages** via
`.github/workflows/deploy-pages.yml` on every push to `main` (and on manual
`workflow_dispatch`).

The axum server under `crates/rsteg-web/` — which serves the real interactive
embed / extract / inspect demo against the Rust library — is the local-dev path
only; it is not deployed.

## One-time setup

Before the first deploy succeeds, a maintainer must:

1. **Create a Cloudflare Pages project named `rsteg`.** In the Cloudflare
   dashboard → Workers & Pages → Create → Pages → "Upload assets" → name it
   `rsteg`. Do *not* connect it to the GitHub repo — this workflow pushes
   assets via `wrangler pages deploy`, which is the "Direct Upload" mode.
   `wrangler pages deploy` requires the project to already exist; if it
   doesn't, the first CI run will fail with a 404 from the Pages API.

2. **Add two repo secrets** in GitHub → Settings → Secrets and variables →
   Actions:

   | Name                     | Where to get it |
   |--------------------------|-----------------|
   | `CLOUDFLARE_API_TOKEN`   | Cloudflare dashboard → My Profile → API Tokens → Create Token → "Edit Cloudflare Workers" template (scope to the right account/zone). |
   | `CLOUDFLARE_ACCOUNT_ID`  | Cloudflare dashboard → Workers & Pages overview, right sidebar. |

   The workflow references these as `${{ secrets.CLOUDFLARE_API_TOKEN }}` and
   `${{ secrets.CLOUDFLARE_ACCOUNT_ID }}` — if you rename the secrets, update
   the workflow too.

## What gets deployed

`public/` is the deploy root. Currently it's a single `index.html` (inline CSS,
inline JS, no external assets). Adding CSS/JS files later is fine — the
workflow uploads the whole directory.

## Updating the site

The source of truth for content is `crates/rsteg-web/src/index.html`. When you
edit that file, re-sync the static export:

- Copy it over to `public/index.html`, then
- **Remove** the three `<div class="panel">` sections for Embed / Extract /
  Inspect, and
- **Remove** the bottom `<script>` that talks to `/api/*`.

Keep the feature-tracker matrix, its summary-count script, the explainer, the
supply-chain posture, and the "how rsteg is built" sections — those all work
offline against the local DOM.

Verify locally with:

```sh
python3 -m http.server --directory public
# visit http://127.0.0.1:8000/
```

## Local preview of the full interactive demo

```sh
cargo run -p rsteg-web
# open http://127.0.0.1:3456
```

This boots the axum server with the preset gallery, upload slots, and real
BMP round-trip against `rsteg-bmp` — the JSON APIs (`/api/embed`,
`/api/extract`, `/api/inspect`, `/api/presets`, `/api/preset/:id`) that the
static deploy deliberately omits.
