# Parley Docs UI

Cloudflare Worker + static assets docs site for Parley.

Docs content source lives in the repository root:

```text
../docs/*.md
```

Build step:

- `scripts-build-docs.mjs` parses `../docs/*.md`
- generates `static/generated/docs.json`

Deploy target:

```text
parley.cloudflavor.io
```

## Local preview

```bash
cd ui-docs
node scripts-build-docs.mjs
wrangler dev
```

## Deploy

```bash
cd ui-docs
wrangler deploy
```
