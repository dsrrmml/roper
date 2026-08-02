#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

tmp_dir=""

package_path="${1:-}"
if [[ -z "$package_path" ]]; then
    package_path="$(find "$repo_root/dist" -maxdepth 1 -type f -name 'roper_*_amd64.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | awk '{print $2}')"
fi

if [[ -z "$package_path" || ! -f "$package_path" ]]; then
    echo "error: package not found" >&2
    exit 1
fi

desktop-file-validate "$repo_root/packaging/debian/roper.desktop"
appstream_output="$(mktemp "$TMPDIR/roper-appstream.XXXXXX")"
trap 'rm -f "$appstream_output"; rm -rf "$tmp_dir"' EXIT
if ! appstreamcli validate --no-net "$repo_root/packaging/debian/org.rmml.roper.metainfo.xml" >"$appstream_output" 2>&1; then
    if grep -q 'url-homepage-missing' "$appstream_output" && ! grep -q '^E:' "$appstream_output"; then
        cat "$appstream_output"
        echo "warning: AppStream validation reported only missing homepage metadata; continuing without inventing a URL" >&2
    else
        cat "$appstream_output" >&2
        exit 1
    fi
else
    cat "$appstream_output"
fi

dpkg-deb --info "$package_path"
dpkg-deb --contents "$package_path"

tmp_dir="$(mktemp -d "$TMPDIR/roper-validate.XXXXXX")"

dpkg-deb --extract "$package_path" "$tmp_dir"

tree_validation_output="$(mktemp "$TMPDIR/roper-appstream-tree.XXXXXX")"
if ! appstreamcli validate-tree "$tmp_dir" >"$tree_validation_output" 2>&1; then
    if grep -q 'url-homepage-missing' "$tree_validation_output" && ! grep -q '^E:' "$tree_validation_output"; then
        cat "$tree_validation_output"
        echo "warning: AppStream tree validation reported only missing homepage metadata; continuing without inventing a URL" >&2
    else
        cat "$tree_validation_output" >&2
        exit 1
    fi
else
    cat "$tree_validation_output"
fi

test -x "$tmp_dir/usr/bin/roper"
test -f "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"
test ! -f "$tmp_dir/usr/share/applications/roper.desktop"
test -f "$tmp_dir/usr/share/metainfo/org.rmml.roper.metainfo.xml"
test -f "$tmp_dir/usr/share/icons/hicolor/scalable/apps/org.rmml.roper.svg"
test -f "$tmp_dir/usr/share/icons/hicolor/256x256/apps/org.rmml.roper.png"
test -f "$tmp_dir/usr/share/roper/splash.jpg"

if find "$tmp_dir" -path '*/docs/*' | grep -q .; then
    echo "error: docs directory unexpectedly present in package payload" >&2
    exit 1
fi

if ! grep -q '^Terminal=false$' "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"; then
    echo "error: desktop file must set Terminal=false" >&2
    exit 1
fi
if ! grep -q '^Exec=roper$' "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"; then
    echo "error: desktop file Exec must be roper" >&2
    exit 1
fi
if ! grep -q '^Icon=org.rmml.roper$' "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"; then
    echo "error: desktop file Icon must match the application id" >&2
    exit 1
fi
if ! grep -q '^StartupWMClass=org.rmml.roper$' "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"; then
    echo "error: desktop file StartupWMClass must match the application id" >&2
    exit 1
fi
if grep -q "$repo_root" "$tmp_dir/usr/share/applications/org.rmml.roper.desktop"; then
    echo "error: desktop file must not reference repository path" >&2
    exit 1
fi
if ! grep -q '<id>org.rmml.roper.desktop</id>' "$tmp_dir/usr/share/metainfo/org.rmml.roper.metainfo.xml"; then
    echo "error: AppStream component id must match the desktop identity" >&2
    exit 1
fi
if ! grep -q '<launchable type="desktop-id">org.rmml.roper.desktop</launchable>' "$tmp_dir/usr/share/metainfo/org.rmml.roper.metainfo.xml"; then
    echo "error: AppStream launchable desktop id mismatch" >&2
    exit 1
fi
if ! grep -q '<pkgname>roper</pkgname>' "$tmp_dir/usr/share/metainfo/org.rmml.roper.metainfo.xml"; then
    echo "error: AppStream metadata must bind to the roper package name" >&2
    exit 1
fi
if ! grep -q '<icon type="stock">org.rmml.roper</icon>' "$tmp_dir/usr/share/metainfo/org.rmml.roper.metainfo.xml"; then
    echo "error: AppStream stock icon must match the application id" >&2
    exit 1
fi
if grep -R -q "$repo_root" "$tmp_dir/usr/share"; then
    echo "error: packaged text files contain absolute repository path" >&2
    exit 1
fi
escaped_repo_root="$(printf '%s\n' "$repo_root" | sed 's/[][(){}.^$+*?|\\]/\\&/g')"
if strings "$tmp_dir/usr/bin/roper" | grep -E "${escaped_repo_root}|${escaped_repo_root}/target" >/dev/null; then
    echo "error: binary appears to reference repository development paths" >&2
    exit 1
fi
if ldd "$tmp_dir/usr/bin/roper" | grep 'not found' >/dev/null; then
    echo "error: packaged binary has unresolved shared libraries" >&2
    exit 1
fi

echo "package validation passed: $package_path"
