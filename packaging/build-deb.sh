#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required" >&2
    exit 1
fi
if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "error: dpkg-deb is required" >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "error: python3 is required" >&2
    exit 1
fi

architecture="$(dpkg --print-architecture)"
if [[ "$architecture" != "amd64" ]]; then
    echo "error: Debian package build is limited to amd64, got '$architecture'" >&2
    exit 1
fi

cargo_toml_path="$repo_root/Cargo.toml"
package_name="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["package"]["name"])
PY
)"
package_version="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["package"]["version"])
PY
)"
binary_name="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
package_name = data["package"]["name"]
bins = data.get("bin") or []
if bins:
    print(bins[0].get("name", package_name))
else:
    print(package_name)
PY
)"
build_stamp="$(date -u +%Y%m%d%H%M%S)"
deb_version="${DEB_VERSION:-${package_version}+local${build_stamp}}"

derive_maintainer() {
    if [[ -n "${DEB_MAINTAINER:-}" ]]; then
        printf '%s' "$DEB_MAINTAINER"
        return 0
    fi

    local name="${DEB_MAINTAINER_NAME:-}"
    local email="${DEB_MAINTAINER_EMAIL:-}"

    if [[ -z "$name" ]]; then
        name="$(git config --get user.name 2>/dev/null || true)"
    fi
    if [[ -z "$email" ]]; then
        email="$(git config --get user.email 2>/dev/null || true)"
    fi
    if [[ -n "$name" && -n "$email" ]]; then
        printf '%s <%s>' "$name" "$email"
        return 0
    fi

    local last_commit_identity
    last_commit_identity="$(git log -1 --format='%an <%ae>' 2>/dev/null || true)"
    if [[ "$last_commit_identity" == *"<"*">"* ]]; then
        printf '%s' "$last_commit_identity"
        return 0
    fi

    if [[ -n "$name" ]]; then
        printf '%s' "$name"
        return 0
    fi

    local author
    author="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
authors = data.get("package", {}).get("authors", [])
print(authors[0] if authors else "")
PY
)"
    if [[ -n "$author" ]]; then
        printf '%s' "$author"
        return 0
    fi

    printf '%s' 'ROPER Project'
}

derive_homepage() {
    if [[ -n "${DEB_HOMEPAGE_URL:-}" ]]; then
        printf '%s' "$DEB_HOMEPAGE_URL"
        return 0
    fi

    local cargo_homepage
    cargo_homepage="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data.get("package", {}).get("homepage", ""))
PY
)"
    if [[ "$cargo_homepage" =~ ^https?:// ]]; then
        printf '%s' "$cargo_homepage"
        return 0
    fi

    local cargo_repository
    cargo_repository="$(python3 - "$cargo_toml_path" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
print(data.get("package", {}).get("repository", ""))
PY
)"
    if [[ "$cargo_repository" =~ ^https?:// ]]; then
        printf '%s' "${cargo_repository%.git}"
        return 0
    fi

    local remote_url
    remote_url="$(git remote get-url origin 2>/dev/null || true)"
    if [[ "$remote_url" =~ ^https?:// ]]; then
        printf '%s' "${remote_url%.git}"
        return 0
    fi
    if [[ "$remote_url" =~ ^git@github\.com:(.+)\.git$ ]]; then
        printf 'https://github.com/%s' "${BASH_REMATCH[1]}"
        return 0
    fi
    if [[ "$remote_url" =~ ^git@gitlab\.com:(.+)\.git$ ]]; then
        printf 'https://gitlab.com/%s' "${BASH_REMATCH[1]}"
        return 0
    fi
    if [[ "$remote_url" =~ ^git@codeberg\.org:(.+)\.git$ ]]; then
        printf 'https://codeberg.org/%s' "${BASH_REMATCH[1]}"
        return 0
    fi
}

maintainer="$(derive_maintainer)"
homepage_url="$(derive_homepage || true)"
appstream_media_base_url="${APPSTREAM_MEDIA_BASE_URL:-}"
appstream_default_screenshot_url="${APPSTREAM_SCREENSHOT_DEFAULT_URL:-}"
appstream_splash_screenshot_url="${APPSTREAM_SCREENSHOT_SPLASH_URL:-}"

if [[ -n "$appstream_media_base_url" ]]; then
    appstream_media_base_url="${appstream_media_base_url%/}"
    if [[ -z "$appstream_default_screenshot_url" ]]; then
        appstream_default_screenshot_url="$appstream_media_base_url/roper-metadata.png"
    fi
    if [[ -z "$appstream_splash_screenshot_url" ]]; then
        appstream_splash_screenshot_url="$appstream_media_base_url/roper-splash.png"
    fi
fi

remap_flags=("--remap-path-prefix=$repo_root=/workspace")
if [[ -d "$HOME/.cargo" ]]; then
    remap_flags+=("--remap-path-prefix=$HOME/.cargo=/cargo-home")
fi
if [[ -d /usr/src ]]; then
    remap_flags+=("--remap-path-prefix=/usr/src=/rust-src")
fi
rustflags_extra="${remap_flags[*]}"
if [[ -n "${RUSTFLAGS:-}" ]]; then
    export RUSTFLAGS="$RUSTFLAGS $rustflags_extra"
else
    export RUSTFLAGS="$rustflags_extra"
fi

if [[ "$package_name" != "roper" ]]; then
    echo "error: expected package name 'roper', got '$package_name'" >&2
    exit 1
fi

package_target_dir="$repo_root/.tmp/package-target"

derive_runtime_depends() {
    local binary_path="$1"
    python3 - "$binary_path" <<'PY'
import subprocess
import sys
from pathlib import Path

binary_path = sys.argv[1]
ldd = subprocess.check_output(["ldd", binary_path], text=True)
packages = []
seen = set()
for line in ldd.splitlines():
    if "=>" not in line:
        continue
    parts = line.split("=>", 1)[1].strip().split()
    if not parts:
        continue
    lib_path = parts[0]
    if lib_path == "not":
        continue
    lib_path = str(Path(lib_path).resolve())
    query = subprocess.check_output(["dpkg", "-S", lib_path], text=True, stderr=subprocess.DEVNULL)
    package = query.split(":", 1)[0].strip()
    if package and package not in seen:
        seen.add(package)
        packages.append(package)
print(", ".join(packages))
PY
}

rm -rf "$package_target_dir"
offline_build_log="$(mktemp "$TMPDIR/roper-build-offline.XXXXXX")"
if ! cargo build --release --locked --offline --target-dir "$package_target_dir" >"$offline_build_log" 2>&1; then
    cat "$offline_build_log" >&2
    echo "warning: offline release build failed; retrying with online Cargo resolution" >&2
    cargo build --release --locked --target-dir "$package_target_dir"
else
    cat "$offline_build_log"
fi
rm -f "$offline_build_log"

built_binary="$package_target_dir/release/$binary_name"
if [[ ! -x "$built_binary" ]]; then
    echo "error: expected executable not found at $built_binary" >&2
    exit 1
fi

stage_root="$repo_root/.tmp/deb/root"
package_root="$stage_root/$package_name"
debian_dir="$package_root/DEBIAN"
dist_dir="$repo_root/dist"

rm -rf "$stage_root"
mkdir -p "$debian_dir" "$dist_dir"
mkdir -p \
    "$package_root/usr/bin" \
    "$package_root/usr/share/applications" \
    "$package_root/usr/share/metainfo" \
    "$package_root/usr/share/doc/$package_name" \
    "$package_root/usr/share/icons/hicolor/scalable/apps" \
    "$package_root/usr/share/icons/hicolor/256x256/apps" \
    "$package_root/usr/share/roper/icons"

install -m 0755 "$built_binary" "$package_root/usr/bin/roper"
if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$package_root/usr/bin/roper"
fi
install -m 0644 "$repo_root/packaging/debian/roper.desktop" "$package_root/usr/share/applications/org.rmml.roper.desktop"
install -m 0644 "$repo_root/packaging/debian/org.rmml.roper.metainfo.xml" "$package_root/usr/share/metainfo/org.rmml.roper.metainfo.xml"
install -m 0644 "$repo_root/packaging/debian/copyright" "$package_root/usr/share/doc/$package_name/copyright"
install -m 0644 "$repo_root/packaging/assets/roper.svg" "$package_root/usr/share/icons/hicolor/scalable/apps/org.rmml.roper.svg"
install -m 0644 "$repo_root/packaging/assets/roper-256.png" "$package_root/usr/share/icons/hicolor/256x256/apps/org.rmml.roper.png"
install -m 0644 "$repo_root/packaging/assets/roper.svg" "$package_root/usr/share/icons/hicolor/scalable/apps/roper.svg"
install -m 0644 "$repo_root/packaging/assets/roper-256.png" "$package_root/usr/share/icons/hicolor/256x256/apps/roper.png"
install -m 0644 "$repo_root/src/resources/splash.jpg" "$package_root/usr/share/roper/splash.jpg"
find "$repo_root/src/resources/icons" -maxdepth 1 -type f -name '*.svg' -exec install -m 0644 {} "$package_root/usr/share/roper/icons/" \;

ROPER_BUILD_MAINTAINER="$maintainer" \
ROPER_BUILD_HOMEPAGE="$homepage_url" \
ROPER_BUILD_SCREENSHOT_DEFAULT="$appstream_default_screenshot_url" \
ROPER_BUILD_SCREENSHOT_SPLASH="$appstream_splash_screenshot_url" \
python3 - "$package_root/usr/share/metainfo/org.rmml.roper.metainfo.xml" <<'PY'
import os
import re
import sys
import xml.etree.ElementTree as ET

metainfo_path = sys.argv[1]
tree = ET.parse(metainfo_path)
root = tree.getroot()

maintainer = os.environ.get("ROPER_BUILD_MAINTAINER", "")
homepage = os.environ.get("ROPER_BUILD_HOMEPAGE", "")
default_screenshot = os.environ.get("ROPER_BUILD_SCREENSHOT_DEFAULT", "")
splash_screenshot = os.environ.get("ROPER_BUILD_SCREENSHOT_SPLASH", "")

def has_http_url(value: str) -> bool:
    return value.startswith("https://") or value.startswith("http://")

match = re.search(r"<([^>]+@[^>]+)>", maintainer)
if match and root.find("update_contact") is None:
    update = ET.Element("update_contact")
    update.text = match.group(1)
    root.insert(4, update)

if has_http_url(homepage) and not any(url.get("type") == "homepage" for url in root.findall("url")):
    url = ET.Element("url", {"type": "homepage"})
    url.text = homepage
    insert_at = next((index for index, child in enumerate(list(root)) if child.tag == "categories"), len(root))
    root.insert(insert_at, url)

screenshots = root.find("screenshots")
if screenshots is None and (has_http_url(default_screenshot) or has_http_url(splash_screenshot)):
    screenshots = ET.Element("screenshots")
    insert_at = next((index for index, child in enumerate(list(root)) if child.tag == "categories"), len(root))
    root.insert(insert_at, screenshots)

if screenshots is not None and len(list(screenshots)) == 0:
    if has_http_url(default_screenshot):
        screenshot = ET.SubElement(screenshots, "screenshot", {"type": "default"})
        image = ET.SubElement(screenshot, "image")
        image.text = default_screenshot
        caption = ET.SubElement(screenshot, "caption")
        caption.text = "ROPER workspace overview"
    if has_http_url(splash_screenshot):
        screenshot = ET.SubElement(screenshots, "screenshot")
        image = ET.SubElement(screenshot, "image")
        image.text = splash_screenshot
        caption = ET.SubElement(screenshot, "caption")
        caption.text = "ROPER splash screen"

ET.indent(tree, space="  ")
tree.write(metainfo_path, encoding="UTF-8", xml_declaration=True)
PY

if find "$package_root" -path '*/docs/*' | grep -q .; then
    echo "error: documentation working files were staged unexpectedly" >&2
    exit 1
fi

echo "determining runtime dependencies from linked shared libraries"
shlibs_depends="$(derive_runtime_depends "$package_root/usr/bin/roper")"
if [[ -z "$shlibs_depends" ]]; then
    echo "error: failed to determine runtime dependencies from linked shared libraries" >&2
    exit 1
fi

installed_size="$(du -sk "$package_root" | awk '{print $1}')"

{
    printf 'Package: roper\n'
    printf 'Version: %s\n' "$deb_version"
    printf 'Section: editors\n'
    printf 'Priority: optional\n'
    printf 'Architecture: amd64\n'
    printf 'Depends: %s\n' "$shlibs_depends"
    printf 'Installed-Size: %s\n' "$installed_size"
    printf 'Maintainer: %s\n' "$maintainer"
    if [[ -n "$homepage_url" ]]; then
        printf 'Homepage: %s\n' "$homepage_url"
    fi
    printf 'Description: Modern dual-pane rap lyric editor\n'
    printf ' Local GTK 4 desktop application for writing, organizing, and refining rap lyrics.\n'
    printf ' Built for offline local use on Debian Trixie.\n'
} > "$debian_dir/control"

find "$dist_dir" -maxdepth 1 -type f -name "${package_name}_*_amd64.deb" -delete
package_path="$dist_dir/${package_name}_${deb_version}_amd64.deb"
dpkg-deb --root-owner-group --build "$package_root" "$package_path"

echo "runtime dependencies: $shlibs_depends"
echo "package built: $package_path"
