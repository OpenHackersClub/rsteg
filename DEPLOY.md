# Deploying the demo site

The `rsteg` demo site lives at `public/` and is published to **Cloudflare Pages**
via `.github/workflows/deploy-pages.yml` on every push to `main` (and on manual
`workflow_dispatch`). Every PR gets a branch-preview URL posted as a comment.

The site is static — plain HTML/CSS/JS, no backend. The live embed / extract
widgets on `/algo/lsb-permuted` and `/algo/payload-header` are currently
disabled; they'll return when `rsteg-wasm` (spec 10) can back them
client-side.

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

`public/` is the deploy root:

- `public/index.html` — landing page (inline CSS/JS, no external assets).
- `public/algo/{index,lsb-linear,lsb-permuted,payload-header,aead}/index.html` —
  algorithm explainer pages.
- `public/benchmarks/` — benchmark results.
- `public/sample/` — Munch-demo cover / stego / payload fixtures.

The workflow uploads the whole directory. Adding more static files is fine.

## Updating content

The intro + LSB basics + bench headline prose is generated from source
READMEs by `rsteg-site-build`:

```sh
cargo run -p rsteg-site-build
```

…which splices marked sections into `public/index.html` and
`public/benchmarks/index.html`. CI runs the same command and
`git diff --exit-code` to assert READMEs and site are in sync.

The algorithm explainer pages under `public/algo/*/index.html` are edited
directly.

## Local preview

```sh
python3 -m http.server --directory public 8787
# open http://127.0.0.1:8787/
```

Matches the deployed artifact byte-for-byte.
