# Linux releases on GitLab

A `v*` tag pipeline builds a signed OSTree repository and a `LegionLight.flatpak`
bundle. The manual `publish-release` job deploys the repository to GitLab Pages at
`https://kestermckinney.gitlab.io/legionlight/flatpak/`.

## One-time setup

1. Make the project **public** (the Flatpak app ID
   `io.gitlab.kestermckinney.LegionLight` must resolve to
   `https://gitlab.com/kestermckinney/LegionLight`; the Flathub linter checks it).
2. Enable GitLab Pages with public access and disable **Use unique domain** so the
   stable URL above serves the repository without redirects.
3. Protect `v*` tags and add protected CI/CD variables:

   | Variable | Type | Contents |
   | --- | --- | --- |
   | `FLATPAK_GPG_PRIVATE_KEY` | File | `.release-secrets/flatpak-signing-private.asc` |
   | `FLATPAK_GPG_KEY_ID` | Variable | `.release-secrets/fingerprint.txt` |
   | `FLATPAK_GPG_PASSPHRASE` | Masked variable | Only if the key has a passphrase (this one has none) |

   `.release-secrets/` is git-ignored and holds the private key and revocation
   certificate. Back it up somewhere safe; never commit it. The matching public key
   is embedded in `legionlight.flatpakrepo`, and CI refuses to sign with any other key.
4. Use a runner that can run Flatpak/bubblewrap builds (a self-managed Docker runner
   may need privileged mode).

## Cut a release

1. Bump `version` in `Cargo.toml` and add a `<release>` to
   `packaging/linux/io.gitlab.kestermckinney.LegionLight.metainfo.xml`.
2. Commit, push, then tag: `git tag v0.1.0 && git push origin v0.1.0`.
   CI checks that the tag matches `Cargo.toml`.
3. Review the `flatpak-release` artifacts, then run `publish-release`.

## Local build

```bash
./packaging/linux/build-flatpak.sh setup          # once
LL_REF=local ./packaging/linux/build-flatpak.sh install   # build working tree and install
flatpak run io.gitlab.kestermckinney.LegionLight
```

`LL_REF` selects the source: `latest` tag (default), `local` working tree, `pinned`
manifest commit, or any tag/branch/SHA. For a signed local repo set `GPG_KEY_ID`,
`GPG_HOMEDIR`, `APP_REPO_URL`, then run `repo` and `bundle`.

## Installing

```bash
flatpak remote-add --user --if-not-exists legionlight \
  https://kestermckinney.gitlab.io/legionlight/flatpak/legionlight.flatpakrepo
flatpak install legionlight io.gitlab.kestermckinney.LegionLight
```

## Keyboard access (host udev rule)

A Flatpak cannot install udev rules. The sandbox gets `/dev/hidraw*` through
`--device=all`, but the host must still allow your session to open the keyboard:

```bash
sudo install -m644 70-legionlight.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Copy the rule from `packaging/70-legionlight.rules` in the repository, or from
`/var/lib/flatpak/app/io.gitlab.kestermckinney.LegionLight/current/active/files/share/legionlight/`
after installing.

## Known limits

- Crates are downloaded during the Flatpak build (`--share=network`). Flathub would
  require offline sources from `flatpak-cargo-generator`.
- The Flathub linter flags the `--device=all` permission; the self-hosted repo has
  no such gate.
