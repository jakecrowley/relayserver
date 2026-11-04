# One-time setup for the Sileo repo (before the first release)

## 1. Generate a GPG signing key

```sh
gpg --full-generate-key    # RSA, sign-only, no expiration is fine
gpg --export-secret-keys -a KEYID > gpg-private.asc   # BACK THIS UP outside GitHub
```

Losing the private key means re-signing everything or starting a new key.

## 2. Give GitHub Actions the key

```sh
gh secret set GPG_PRIVATE_KEY < gpg-private.asc
gh secret set GPG_PASSPHRASE        # only if the key has a passphrase
```

## 3. Enable GitHub Pages

Settings → Pages → Source: `gh-pages` branch. The branch doesn't exist until
the first tag run — enable after the first workflow completes.

## 4. Release

```sh
git tag v0.0.4 && git push origin v0.0.4
```

CI builds both `.deb` variants (rootful + rootless), signs the repo metadata,
and publishes to the Pages URL. Old versions stay in the pool — never deleted.

## 5. Verify

- The Actions run goes green.
- `https://jakecrowley.github.io/relayserver/Release` downloads.
- In Sileo: Sources → Add → `https://jakecrowley.github.io/relayserver/`, then
  install `dev.copper.relayserver` (the `iphoneos-arm64` variant on rootless
  jailbreaks).
