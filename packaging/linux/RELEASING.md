# Linux releases on GitHub

GitHub Actions builds a signed Flatpak repository and bundle. GitHub Pages hosts
the repository at `https://kestermckinney.github.io/legionlight/flatpak/`.
The app ID remains `io.gitlab.kestermckinney.LegionLight` so existing installations
can update. The GitLab project is private, so this ID will not pass Flathub's
public ownership checks; this repository is self-hosted, not a Flathub submission.

## Signing and publishing

The repository's Actions secrets `FLATPAK_GPG_PRIVATE_KEY` and
`FLATPAK_GPG_KEY_ID` hold the existing signing key and fingerprint. If the key
has a passphrase, set `FLATPAK_GPG_PASSPHRASE` as well. Keep an independent,
secure backup of the private key. The build checks it against the public key in
`legionlight.flatpakrepo` and stops if they differ.

GitHub Pages must use GitHub Actions as its publishing source. A successful tag
build publishes the Pages repository and attaches the bundle and `.flatpakrepo`
to a GitHub release. Only publish trusted tags: a tag build can deploy the site
and access the signing secrets.

1. Bump the version in `Cargo.toml` and `Cargo.lock`, and add a release to
   `io.gitlab.kestermckinney.LegionLight.metainfo.xml`.
2. Commit and push to `main`, then create and push a matching tag, for example
   `git tag v0.4.0 && git push origin v0.4.0`.
3. Check the Flatpak workflow, GitHub release, and hosted repository. CI rejects
   a tag that doesn't match `Cargo.toml`.

A manually dispatched workflow builds and publishes the version in `Cargo.toml`
without creating a GitHub release.

## Local build

```bash
./packaging/linux/build-flatpak.sh setup
LL_REF=local ./packaging/linux/build-flatpak.sh install
flatpak run io.gitlab.kestermckinney.LegionLight
```

`LL_REF` selects the latest tag (default), `local` working tree, `pinned`
manifest commit, or a tag, branch or SHA. For a signed local repository set
`GPG_KEY_ID`, `GPG_HOMEDIR`, and `APP_REPO_URL`, then run `repo` and `bundle`.
See the README for installation, migration and keyboard access instructions.

The build downloads crates over the network. A Flathub submission would require
offline sources and review of the `--device=all` permission.
