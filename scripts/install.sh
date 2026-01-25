#!/usr/bin/env sh
set -eu

REPO="HarrisDePerceptron/Rust-Oxide"
BINARY="oxide"
API_BASE="https://api.github.com/repos/${REPO}"
RELEASES_BASE="https://github.com/${REPO}/releases/download"

ACTION="install"
REQUESTED_VERSION=""
PREFIX=""
FORCE=0
NO_PATH=0
QUIET=0
DEBUG=0
DRY_RUN=0
STRICT_CHECKSUM=${OXIDE_INSTALL_STRICT:-0}

print_usage() {
  cat <<'USAGE'
Rust Oxide CLI installer

Usage:
  install.sh [options]

Options:
  --update            Update to the latest version if newer is available
  --uninstall         Uninstall the currently installed binary
  --version VERSION   Install a specific version (e.g. 0.3.4 or v0.3.4)
  --prefix DIR        Install to DIR instead of the default
  --force             Overwrite existing binary
  --debug             Print diagnostic information and exit
  --dry-run           Print planned actions without changing anything
  --no-path           Do not attempt to modify PATH or print PATH hints
  --quiet             Reduce output
  --help              Show this help

Environment:
  OXIDE_VERSION         Same as --version
  OXIDE_PREFIX          Same as --prefix
  OXIDE_INSTALL_STRICT  If set to 1, require checksum verification
USAGE
}

log() {
  if [ "$QUIET" -eq 0 ]; then
    printf '%s\n' "$*"
  fi
}

warn() {
  if [ "$QUIET" -eq 0 ]; then
    printf 'warning: %s\n' "$*" >&2
  fi
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

fetch() {
  url=$1
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$url"
  else
    fail "curl or wget is required"
  fi
}

fetch_optional() {
  url=$1
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" 2>/dev/null || return 1
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$url" 2>/dev/null || return 1
  else
    return 1
  fi
}

url_exists() {
  url=$1
  if command -v curl >/dev/null 2>&1; then
    curl -fsI "$url" >/dev/null 2>&1
  elif command -v wget >/dev/null 2>&1; then
    wget --spider -q "$url" >/dev/null 2>&1
  else
    return 1
  fi
}

download() {
  url=$1
  dest=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
  else
    fail "curl or wget is required"
  fi
}

normalize_version() {
  case "$1" in
    v*) printf '%s\n' "$1" ;;
    *) printf 'v%s\n' "$1" ;;
  esac
}

latest_tag() {
  fetch "${API_BASE}/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

latest_tag_optional() {
  if response=$(fetch_optional "${API_BASE}/releases/latest"); then
    printf '%s' "$response" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
  fi
}

current_version() {
  if command -v "$BINARY" >/dev/null 2>&1; then
    "$BINARY" --version 2>/dev/null | sed -n 's/.*\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' | head -n 1
  fi
}

sha256() {
  file=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    fail "sha256sum or shasum is required to verify checksums"
  fi
}

os_arch_target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-gnu" ;;
    *) fail "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  printf '%s-%s\n' "$arch" "$os"
}

os_arch_target_optional() {
  os=$(uname -s 2>/dev/null || printf '')
  arch=$(uname -m 2>/dev/null || printf '')

  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-gnu" ;;
    *) printf '%s' ""; return 0 ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) printf '%s' ""; return 0 ;;
  esac

  printf '%s-%s\n' "$arch" "$os"
}

choose_install_dir() {
  if [ -n "$PREFIX" ]; then
    printf '%s\n' "$PREFIX"
    return
  fi

  if [ "$ACTION" = "update" ] && command -v "$BINARY" >/dev/null 2>&1; then
    dirname "$(command -v "$BINARY")"
    return
  fi

  if [ "$(id -u)" -eq 0 ]; then
    printf '%s\n' "/usr/local/bin"
    return
  fi

  if [ -w "/usr/local/bin" ]; then
    printf '%s\n' "/usr/local/bin"
    return
  fi

  if command -v sudo >/dev/null 2>&1; then
    printf '%s\n' "/usr/local/bin"
    return
  fi

  if [ -w "${HOME}/.local" ] || mkdir -p "${HOME}/.local/bin" 2>/dev/null; then
    printf '%s\n' "${HOME}/.local/bin"
    return
  fi

  if [ -w "${HOME}" ] || mkdir -p "${HOME}/bin" 2>/dev/null; then
    printf '%s\n' "${HOME}/bin"
    return
  fi

  fail "could not determine a writable install directory"
}

needs_sudo() {
  dir=$1
  if [ "$(id -u)" -eq 0 ]; then
    return 1
  fi
  if [ -d "$dir" ]; then
    [ -w "$dir" ] && return 1
  else
    parent=$(dirname "$dir")
    [ -w "$parent" ] && return 1
  fi
  command -v sudo >/dev/null 2>&1
}

can_write_dir() {
  dir=$1
  if [ -d "$dir" ]; then
    [ -w "$dir" ]
  else
    parent=$(dirname "$dir")
    [ -w "$parent" ]
  fi
}

run_as_root() {
  if needs_sudo "$INSTALL_DIR"; then
    sudo "$@"
  else
    "$@"
  fi
}

ensure_path_hint() {
  if [ "$NO_PATH" -eq 1 ]; then
    return
  fi

  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return ;;
  esac

  case "$INSTALL_DIR" in
    /usr/local/bin|/usr/bin|/bin|/sbin|/usr/sbin|/opt/homebrew/bin) return ;;
  esac

  warn "${INSTALL_DIR} is not on your PATH. Add it to your shell config to use '$BINARY'."
}

diagnostics() {
  detected_target=""
  detected_target=$(os_arch_target_optional 2>/dev/null || true)
  detected_target=${detected_target:-unknown}

  installed_path=""
  if command -v "$BINARY" >/dev/null 2>&1; then
    installed_path=$(command -v "$BINARY")
  fi

  current=""
  current=$(current_version || true)
  current=${current:-not installed}

  latest=""
  latest=$(latest_tag_optional 2>/dev/null || true)
  latest=${latest:-unknown}

  tag_to_check="$latest"
  if [ -n "$REQUESTED_VERSION" ]; then
    tag_to_check=$(normalize_version "$REQUESTED_VERSION")
  fi

  supported="unknown"
  asset_url="unknown"
  if [ "$detected_target" != "unknown" ] && [ -n "$tag_to_check" ] && [ "$tag_to_check" != "unknown" ]; then
    asset_url="${RELEASES_BASE}/${tag_to_check}/${BINARY}-${tag_to_check}-${detected_target}.tar.gz"
    if url_exists "$asset_url"; then
      supported="yes"
    else
      supported="no"
    fi
  fi

  log "Repository: ${REPO}"
  log "Detected target: ${detected_target}"
  log "Installed binary: ${installed_path:-not found}"
  log "Current version: ${current}"
  log "Latest version: ${latest}"
  log "Asset URL: ${asset_url}"
  log "Target supported: ${supported}"
}

dry_run_plan() {
  TARGET=$(os_arch_target)
  if [ -n "$REQUESTED_VERSION" ]; then
    TAG=$(normalize_version "$REQUESTED_VERSION")
  else
    TAG=$(latest_tag)
    [ -n "$TAG" ] || fail "could not determine latest version"
  fi

  ARCHIVE="${BINARY}-${TAG}-${TARGET}.tar.gz"
  URL="${RELEASES_BASE}/${TAG}/${ARCHIVE}"
  INSTALL_DIR=$(choose_install_dir)
  NEEDS_SUDO="no"
  if needs_sudo "$INSTALL_DIR"; then
    NEEDS_SUDO="yes"
  fi

  log "Action: ${ACTION}"
  log "Target: ${TARGET}"
  log "Version tag: ${TAG}"
  log "Archive: ${ARCHIVE}"
  log "Download URL: ${URL}"
  log "Install dir: ${INSTALL_DIR}"
  log "Needs sudo: ${NEEDS_SUDO}"
  log "Checksum strict: ${STRICT_CHECKSUM}"
}

uninstall() {
  if [ -n "$PREFIX" ]; then
    target="$PREFIX/$BINARY"
  elif command -v "$BINARY" >/dev/null 2>&1; then
    target="$(command -v "$BINARY")"
  else
    target=""
  fi

  if [ -z "$target" ]; then
    for candidate in "/usr/local/bin" "${HOME}/.local/bin" "${HOME}/bin"; do
      if [ -x "$candidate/$BINARY" ]; then
        target="$candidate/$BINARY"
        break
      fi
    done
  fi

  if [ -z "$target" ] || [ ! -e "$target" ]; then
    fail "${BINARY} is not installed"
  fi

  INSTALL_DIR=$(dirname "$target")
  log "Removing $target"
  run_as_root rm -f "$target"
  log "Uninstalled ${BINARY}"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --update) ACTION="update" ;;
    --uninstall) ACTION="uninstall" ;;
    --version)
      shift
      [ "$#" -gt 0 ] || fail "--version requires a value"
      REQUESTED_VERSION="$1"
      ;;
    --prefix)
      shift
      [ "$#" -gt 0 ] || fail "--prefix requires a value"
      PREFIX="$1"
      ;;
    --force) FORCE=1 ;;
    --debug) DEBUG=1 ;;
    --dry-run) DRY_RUN=1 ;;
    --no-path) NO_PATH=1 ;;
    --quiet) QUIET=1 ;;
    --help|-h) print_usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
  shift
 done

if [ -n "${OXIDE_VERSION:-}" ]; then
  REQUESTED_VERSION="$OXIDE_VERSION"
fi
if [ -n "${OXIDE_PREFIX:-}" ]; then
  PREFIX="$OXIDE_PREFIX"
fi

if [ "$DEBUG" -eq 1 ]; then
  diagnostics
  exit 0
fi

if [ "$DRY_RUN" -eq 1 ]; then
  dry_run_plan
  exit 0
fi

if [ "$ACTION" = "uninstall" ]; then
  uninstall
  exit 0
fi

if [ -n "$REQUESTED_VERSION" ]; then
  TAG=$(normalize_version "$REQUESTED_VERSION")
else
  TAG=$(latest_tag)
  [ -n "$TAG" ] || fail "could not determine latest version"
fi

TARGET=$(os_arch_target)
ARCHIVE="${BINARY}-${TAG}-${TARGET}.tar.gz"
URL="${RELEASES_BASE}/${TAG}/${ARCHIVE}"

if [ "$ACTION" = "update" ]; then
  CURRENT=$(current_version || true)
  LATEST=$(printf '%s' "$TAG" | sed 's/^v//')
  if [ -n "$CURRENT" ] && [ "$CURRENT" = "$LATEST" ]; then
    log "${BINARY} is already at the latest version (${CURRENT})"
    exit 0
  fi
fi

INSTALL_DIR=$(choose_install_dir)
if ! can_write_dir "$INSTALL_DIR" && ! needs_sudo "$INSTALL_DIR"; then
  fail "install directory ${INSTALL_DIR} is not writable and sudo is unavailable"
fi

if [ -e "$INSTALL_DIR/$BINARY" ] && [ "$FORCE" -eq 0 ]; then
  if [ "$ACTION" = "install" ]; then
    log "${BINARY} already exists at ${INSTALL_DIR}/${BINARY}"
    log "Use --force to overwrite or --update to check for a newer version"
    exit 0
  fi
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

log "Downloading ${BINARY} ${TAG} for ${TARGET}"
archive_path="$tmpdir/$ARCHIVE"
download "$URL" "$archive_path"

expected_checksum=""
if checksum_text=$(fetch_optional "${URL}.sha256"); then
  expected_checksum=$(printf '%s' "$checksum_text" | awk '{print $1}' | head -n 1)
elif checksum_text=$(fetch_optional "${RELEASES_BASE}/${TAG}/checksums.txt"); then
  expected_checksum=$(printf '%s' "$checksum_text" | awk -v name="$ARCHIVE" '$2 == name {print $1}' | head -n 1)
fi

if [ -n "$expected_checksum" ]; then
  actual_checksum=$(sha256 "$archive_path")
  if [ "$expected_checksum" != "$actual_checksum" ]; then
    fail "checksum verification failed"
  fi
  log "Checksum verified"
else
  if [ "$STRICT_CHECKSUM" -eq 1 ]; then
    fail "checksum not available for ${ARCHIVE}"
  fi
  warn "checksum not available; proceeding without verification"
fi

log "Extracting archive"
mkdir -p "$tmpdir/extract"
tar -xzf "$archive_path" -C "$tmpdir/extract"

bin_path=$(find "$tmpdir/extract" -type f -name "$BINARY" | head -n 1)
[ -n "$bin_path" ] || fail "could not find ${BINARY} in archive"

log "Installing to ${INSTALL_DIR}/${BINARY}"
run_as_root mkdir -p "$INSTALL_DIR"
if command -v install >/dev/null 2>&1; then
  run_as_root install -m 0755 "$bin_path" "$INSTALL_DIR/$BINARY"
else
  run_as_root cp "$bin_path" "$INSTALL_DIR/$BINARY"
  run_as_root chmod 0755 "$INSTALL_DIR/$BINARY"
fi

log "${BINARY} installed successfully"
ensure_path_hint
