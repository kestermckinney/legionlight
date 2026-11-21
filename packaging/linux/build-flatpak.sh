#!/bin/bash
# Build, lint, export and bundle the Legion Light Flatpak.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$SCRIPT_DIR/flatpak-build"
REPO_DIR="${REPO_DIR:-$SCRIPT_DIR/flatpak-repo}"
BUNDLE_FILE="${BUNDLE_FILE:-$SCRIPT_DIR/LegionLight.flatpak}"
APP_ID="io.gitlab.kestermckinney.LegionLight"
MANIFEST="$SCRIPT_DIR/${APP_ID}.yml"
RUNTIME_VERSION="25.08"

# Optional signing / self-hosting knobs (all opt-in via env).
GPG_KEY_ID="${GPG_KEY_ID:-}"
GPG_HOMEDIR="${GPG_HOMEDIR:-}"
APP_REPO_URL="${APP_REPO_URL:-}"
RUNTIME_REPO_URL="${RUNTIME_REPO_URL:-https://flathub.org/repo/flathub.flatpakrepo}"
# Set to 1 in containerized CI where rofiles-fuse cannot spawn.
DISABLE_ROFILES_FUSE="${DISABLE_ROFILES_FUSE:-0}"
USE_BUILDER_FLATPAK="${USE_BUILDER_FLATPAK:-auto}"

BUILDER_FLAGS=()
[ "$DISABLE_ROFILES_FUSE" = "1" ] && BUILDER_FLAGS+=("--disable-rofiles-fuse")
FLATPAK_BUILDER=(flatpak-builder)
if [ "$USE_BUILDER_FLATPAK" = "1" ] || \
   { [ "$USE_BUILDER_FLATPAK" = "auto" ] && flatpak info org.flatpak.Builder &>/dev/null; }; then
    FLATPAK_BUILDER=(flatpak run --command=flatpak-builder org.flatpak.Builder)
    [ "$DISABLE_ROFILES_FUSE" = "1" ] || BUILDER_FLAGS+=("--disable-rofiles-fuse")
fi

# LL_REF picks the source that gets compiled:
#   latest (default)  newest v* tag on GitHub
#   local             this working tree, uncommitted edits included (for testing)
#   pinned            the manifest's committed `commit:` value, untouched
#   <tag|branch|sha>  that ref
# The resolved pin goes into a generated copy; the committed manifest is not edited.
LL_REF="${LL_REF:-latest}"
LL_GIT_URL="https://github.com/kestermckinney/legionlight.git"
RESOLVED_MANIFEST="$SCRIPT_DIR/.${APP_ID}.resolved.yml"
LINTER_APP="org.flatpak.Builder"
METAINFO_FILE="$SCRIPT_DIR/${APP_ID}.metainfo.xml"
DESKTOP_FILE="$SCRIPT_DIR/${APP_ID}.desktop"

remote_sha() {
    local url="$1" ref="$2" out
    if [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then echo "$ref"; return; fi
    out="$(git ls-remote "$url" "refs/tags/$ref^{}" "refs/tags/$ref" "refs/heads/$ref" "$ref")"
    { grep -F "refs/tags/$ref^{}" <<<"$out" || head -n1 <<<"$out"; } | awk 'NR==1 {print $1}'
}

resolve_manifest() {
    case "$LL_REF" in
    pinned) echo "==> Using committed manifest pin (LL_REF=pinned)"; return ;;
    local)
        # Replace the git source with the working tree (skipping build outputs).
        awk -v dir="$PROJECT_DIR" '
            /^      - type: git/ { print "      - type: dir"; print "        path: " dir; print "        skip: [target, .git, .release-secrets, .flatpak-builder, packaging/linux/flatpak-build, packaging/linux/flatpak-repo]"; skip = 2; next }
            skip > 0 { skip--; next }
            { print }' "$MANIFEST" > "$RESOLVED_MANIFEST"
        MANIFEST="$RESOLVED_MANIFEST"
        echo "==> Building the local working tree (LL_REF=local)"
        return ;;
    esac
    local ref="$LL_REF" sha
    if [ "$ref" = "latest" ]; then
        ref="$(git ls-remote --tags "$LL_GIT_URL" 'v*' | awk '{print $2}' \
            | sed -e 's#^refs/tags/##' -e 's#\^{}$##' | sort -uV | tail -n1)"
        [ -n "$ref" ] || { echo "Error: no v* tags found on $LL_GIT_URL"; exit 1; }
    fi
    sha="$(remote_sha "$LL_GIT_URL" "$ref")"
    [[ "$sha" =~ ^[0-9a-f]{40}$ ]] || { echo "Error: could not resolve '$ref'"; exit 1; }
    sed -E "s/^(        commit: ).*/\1$sha/" "$MANIFEST" > "$RESOLVED_MANIFEST"
    MANIFEST="$RESOLVED_MANIFEST"
    echo "==> Legion Light -> $ref ($sha)"
}

gpg_sign_args() {
    local -n _out="$1"
    _out=()
    if [ -n "$GPG_KEY_ID" ]; then
        _out+=("--gpg-sign=$GPG_KEY_ID")
        [ -n "$GPG_HOMEDIR" ] && _out+=("--gpg-homedir=$GPG_HOMEDIR")
    fi
}

usage() {
    echo "Usage: $0 [setup|build|lint|run|install|repo|bundle|clean]"
    echo ""
    echo "  build/install/repo clone Legion Light from GitHub. By default they build"
    echo "  the newest v* tag. Override with LL_REF=<tag|branch|sha>, LL_REF=local to"
    echo "  build this working tree, or LL_REF=pinned for the manifest's commit."
    echo ""
    echo "  setup    Install flatpak-builder, the runtime/SDK, Rust extension and lint tools"
    echo "  build    Build the Flatpak"
    echo "  lint     Flathub-style validation (manifest, metainfo, desktop); --strict fails on issues"
    echo "  run      Test-run the built Flatpak"
    echo "  install  Install the Flatpak for the current user"
    echo "  repo     Export to a local Flatpak repository (lint runs first)"
    echo "  bundle   Create a standalone .flatpak bundle"
    echo "  clean    Remove build and repo directories"
    exit 1
}

cmd_setup() {
    command -v flatpak-builder &>/dev/null || sudo dnf install -y flatpak-builder
    flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
    flatpak install -y --noninteractive flathub \
        org.freedesktop.Platform//${RUNTIME_VERSION} org.freedesktop.Sdk//${RUNTIME_VERSION} \
        org.freedesktop.Sdk.Extension.rust-stable//${RUNTIME_VERSION} || true
    flatpak install -y --noninteractive flathub "$LINTER_APP" || true
    if ! command -v appstreamcli &>/dev/null || ! command -v desktop-file-validate &>/dev/null; then
        sudo dnf install -y appstream desktop-file-utils || true
    fi
    echo "==> Setup complete."
}

cmd_lint() {
    local strict=0 failures=0
    [ "${1:-}" = "--strict" ] && strict=1
    [ "${STRICT:-0}" = "1" ] && strict=1
    echo "==> Linting..."
    if flatpak info "$LINTER_APP" &>/dev/null; then
        flatpak run --command=flatpak-builder-lint "$LINTER_APP" manifest "$SCRIPT_DIR/${APP_ID}.yml" || failures=$((failures + 1))
    else
        echo "--> SKIP manifest lint (${LINTER_APP} not installed; run '$0 setup')"
    fi
    if command -v appstreamcli &>/dev/null; then
        appstreamcli validate --no-net "$METAINFO_FILE" || failures=$((failures + 1))
    else
        echo "--> SKIP metainfo validation (appstreamcli not found)"
    fi
    if command -v desktop-file-validate &>/dev/null; then
        desktop-file-validate "$DESKTOP_FILE" || failures=$((failures + 1))
    else
        echo "--> SKIP desktop validation (desktop-file-validate not found)"
    fi
    if [ "$failures" -gt 0 ]; then
        echo "==> Lint finished with ${failures} failing check(s)."
        [ "$strict" = "1" ] && return 1
    else
        echo "==> Lint passed."
    fi
    return 0
}

cmd_build() {
    resolve_manifest
    "${FLATPAK_BUILDER[@]}" "${BUILDER_FLAGS[@]}" --force-clean "$BUILD_DIR" "$MANIFEST"
    echo "==> Build complete. Output in $BUILD_DIR"
}

cmd_run() {
    [ -d "$BUILD_DIR" ] || { echo "Error: run '$0 build' first."; exit 1; }
    resolve_manifest
    # flatpak-builder --run leaves WAYLAND_DISPLAY unset; pass the host's through.
    flatpak-builder --run "$BUILD_DIR" "$MANIFEST" env WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" legion_light
}

cmd_install() {
    resolve_manifest
    "${FLATPAK_BUILDER[@]}" "${BUILDER_FLAGS[@]}" --user --install --force-clean "$BUILD_DIR" "$MANIFEST"
    echo "==> Installed. Run with: flatpak run $APP_ID"
}

cmd_repo() {
    [ -d "$BUILD_DIR" ] || { echo "Error: run '$0 build' first."; exit 1; }
    cmd_lint
    resolve_manifest
    local sign_args
    gpg_sign_args sign_args
    "${FLATPAK_BUILDER[@]}" "${BUILDER_FLAGS[@]}" --repo="$REPO_DIR" "${sign_args[@]}" --force-clean "$BUILD_DIR" "$MANIFEST"
    echo "==> Repository created at $REPO_DIR"
}

cmd_bundle() {
    [ -d "$REPO_DIR" ] || { echo "Error: run '$0 repo' first."; exit 1; }
    local extra=() key_file=""
    [ -n "$APP_REPO_URL" ] && extra+=("--repo-url=$APP_REPO_URL")
    if [ -n "$GPG_KEY_ID" ]; then
        key_file="$(mktemp)"
        gpg ${GPG_HOMEDIR:+--homedir "$GPG_HOMEDIR"} --export "$GPG_KEY_ID" > "$key_file"
        extra+=("--gpg-keys=$key_file")
    fi
    [ -n "$RUNTIME_REPO_URL" ] && extra+=("--runtime-repo=$RUNTIME_REPO_URL")
    flatpak build-bundle "${extra[@]}" "$REPO_DIR" "$BUNDLE_FILE" "$APP_ID"
    [ -z "$key_file" ] || rm -f "$key_file"
    echo "==> Bundle created: $BUNDLE_FILE  (install: flatpak install $BUNDLE_FILE)"
}

cmd_clean() {
    rm -rf "$BUILD_DIR" "$REPO_DIR" "$RESOLVED_MANIFEST" "$BUNDLE_FILE"
    echo "==> Clean complete."
}

case "${1:-}" in
    setup)   cmd_setup ;;
    build)   cmd_build ;;
    lint)    cmd_lint "${2:-}" ;;
    run)     cmd_run ;;
    install) cmd_install ;;
    repo)    cmd_repo ;;
    bundle)  cmd_bundle ;;
    clean)   cmd_clean ;;
    *)       usage ;;
esac
