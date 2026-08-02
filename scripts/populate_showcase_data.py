#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import struct
import textwrap
import zlib
from pathlib import Path
from typing import Iterable


def storage_root() -> Path:
    explicit = os.environ.get("ROPER_STORAGE_DIR")
    if explicit:
        return Path(explicit).expanduser().resolve()

    xdg_data = os.environ.get("XDG_DATA_HOME")
    if xdg_data:
        return Path(xdg_data).expanduser().resolve() / "roper"

    home = Path.home()
    return home / ".local" / "share" / "roper"


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def normalize_line(line: str) -> str | None:
    trimmed = line.strip()
    if not trimmed:
        return None

    normalized_chars: list[str] = []
    in_space = False
    for ch in trimmed:
        if ch in "\n\r":
            continue
        if ch.isspace():
            if not in_space:
                normalized_chars.append(" ")
                in_space = True
        else:
            normalized_chars.append(ch)
            in_space = False

    normalized = "".join(normalized_chars).upper().lower()
    return normalized or None


def used_material_entries(raw_lines: Iterable[str], pick_every: int = 12) -> list[dict]:
    occurrence_by_hash: dict[str, int] = {}
    identities: list[tuple[str, int]] = []

    for line in raw_lines:
        normalized = normalize_line(line)
        if not normalized:
            continue
        digest = hashlib.md5(normalized.encode("utf-8")).hexdigest()
        occurrence = occurrence_by_hash.get(digest, 0)
        occurrence_by_hash[digest] = occurrence + 1
        identities.append((digest, occurrence))

    picked: list[dict] = []
    for idx, (digest, occurrence) in enumerate(identities):
        if idx % pick_every == 0:
            picked.append({"normalized_hash": digest, "occurrence": occurrence})

    return picked[:24]


def romanian_line(seed: int, mood: str, district: str, image: str) -> str:
    openers = [
        "Scriu pe trotuar",
        "Țin ritmul în piept",
        "Noaptea respiră",
        "Ploaia bate-n geam",
        "Cartierul îmi spune",
        "Microfonul tace",
        "Lampadarul arde",
        "Nervul meu pulsează",
    ]
    verbs = [
        "adun", "aprind", "strâng", "sparg", "ridic", "cânt", "țes", "mut", "scrijelesc", "modelez"
    ]
    nouns = [
        "silabe", "ecouri", "adevăr", "rime", "pași", "umbre", "foc", "liniște", "foșnet", "metafore"
    ]
    endings = [
        "până răsare soarele",
        "pe asfaltul ud",
        "în ritm de inimă",
        "ca o sirenă lentă",
        "printre neoane reci",
        "în aer de duminică",
        "cu dinții strânși",
        "fără mască, fără frână",
    ]

    opener = openers[seed % len(openers)]
    verb = verbs[(seed * 3 + 5) % len(verbs)]
    noun = nouns[(seed * 7 + 11) % len(nouns)]
    ending = endings[(seed * 13 + 17) % len(endings)]
    return f"{opener}, {verb} {noun} {mood} din {district}, {image}, {ending}."


def chorus_line(seed: int, hook: str) -> str:
    variants = [
        f"{hook} și bat din palme pe contră.",
        f"{hook}, ecoul urcă peste blocuri.",
        f"{hook}, respir greu dar nu cedez.",
        f"{hook}, îmi ține coloana dreaptă.",
    ]
    return variants[seed % len(variants)]


def build_track_texts(track_title: str, mood: str, district: str, hook_phrase: str) -> tuple[str, str]:
    final_lines: list[str] = []
    raw_lines: list[str] = []

    final_lines.append("[INTRO]")
    for i in range(1, 17):
        final_lines.append(romanian_line(i, mood, district, "intro aprins"))

    for verse in range(1, 6):
        final_lines.append(f"[VERSE {verse}]")
        for i in range(1, 31):
            seed = verse * 200 + i
            final_lines.append(romanian_line(seed, mood, district, f"vers {verse}"))

        final_lines.append(f"[HOOK {verse}]")
        for i in range(1, 13):
            final_lines.append(chorus_line(verse * 50 + i, hook_phrase))

        if verse in (2, 4):
            final_lines.append(f"[BRIDGE {verse // 2}]")
            for i in range(1, 10):
                final_lines.append(
                    f"Podul {verse // 2} ține pulsul strâns, {romanian_line(verse * 80 + i, mood, district, 'pod electric')}"
                )

    final_lines.append("[OUTRO]")
    for i in range(1, 14):
        final_lines.append(
            f"Închid {track_title.lower()} cu pas calm: {romanian_line(900 + i, mood, district, 'outro adânc')}"
        )

    # raw pane = larger dump, variants, duplicates for highlight buckets, and transferable chunks
    raw_lines.extend(
        [
            "[RAW STACK A]",
            "rescriu același impuls până iese clar",
            "rescriu același impuls până iese clar",
            "rescriu același impuls până iese clar",
            "rescriu același impuls până iese clar",
            "rescriu același impuls până iese clar",
            "",
        ]
    )

    for block in range(1, 11):
        raw_lines.append(f"[RAW BLOCK {block}]")
        for i in range(1, 26):
            seed = block * 300 + i
            base = romanian_line(seed, mood, district, f"schiță {block}")
            raw_lines.append(base)
            if i % 8 == 0:
                raw_lines.append(chorus_line(seed, hook_phrase))

    raw_lines.extend(
        [
            "[RAW STACK B]",
            "aceeași imagine, aceeași rană, aceeași ramă",
            "aceeași imagine, aceeași rană, aceeași ramă",
            "aceeași imagine, aceeași rană, aceeași ramă",
            "aceeași imagine, aceeași rană, aceeași ramă",
            "",
        ]
    )

    # keep final >= 200 lines and raw >= 200 lines
    if len(final_lines) < 200:
        raise RuntimeError("Final pane generation produced too few lines.")
    if len(raw_lines) < 200:
        raise RuntimeError("Raw pane generation produced too few lines.")

    return "\n".join(final_lines) + "\n", "\n".join(raw_lines) + "\n"


def write_png(path: Path, width: int, height: int, rgb_rows: list[list[tuple[int, int, int]]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = bytearray()
    for row in rgb_rows:
        raw.append(0)
        for r, g, b in row:
            raw.extend((r, g, b))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = bytearray(b"\x89PNG\r\n\x1a\n")
    png.extend(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
    png.extend(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
    png.extend(chunk(b"IEND", b""))
    path.write_bytes(png)


def gradient_image(path: Path, width: int, height: int, base: tuple[int, int, int], accent: tuple[int, int, int]) -> None:
    rows: list[list[tuple[int, int, int]]] = []
    for y in range(height):
        row: list[tuple[int, int, int]] = []
        for x in range(width):
            fx = x / max(1, width - 1)
            fy = y / max(1, height - 1)
            blend = (fx * 0.65 + fy * 0.35)
            r = int(base[0] * (1 - blend) + accent[0] * blend)
            g = int(base[1] * (1 - blend) + accent[1] * blend)
            b = int(base[2] * (1 - blend) + accent[2] * blend)

            if (x // 24 + y // 24) % 2 == 0:
                r = min(255, r + 10)
                g = min(255, g + 10)
                b = min(255, b + 10)
            row.append((r, g, b))
        rows.append(row)
    write_png(path, width, height, rows)


def screenshot_mock(path: Path, palette: tuple[tuple[int, int, int], tuple[int, int, int], tuple[int, int, int]]) -> None:
    width, height = 1365, 768
    bg, pane_left, pane_right = palette
    rows: list[list[tuple[int, int, int]]] = []
    for y in range(height):
        row: list[tuple[int, int, int]] = []
        for x in range(width):
            color = bg
            if 0 <= y < 40:
                color = (42, 45, 52)
            elif 80 <= y < 744 and 40 <= x < 530:
                color = pane_left
            elif 80 <= y < 744 and 560 <= x < 1320:
                color = pane_right
            elif 80 <= y < 744 and 530 <= x < 560:
                color = (30, 32, 38)
            elif 80 <= y < 744 and 1320 <= x < 1355:
                color = (26, 30, 35)

            if 120 <= y < 730 and (y % 17 == 0) and 70 <= x < 1300:
                color = (min(255, color[0] + 26), min(255, color[1] + 14), min(255, color[2] + 10))

            if 740 <= y < 768:
                color = (35, 37, 43)

            row.append(color)
        rows.append(row)
    write_png(path, width, height, rows)


def idea_text(prefix: str, lines: int) -> str:
    blocks = []
    for i in range(1, lines + 1):
        blocks.append(
            f"{prefix} {i:03d}: păstrez nervul viu, schimb măsura, tai zgomotul, întorc fraza și las ideea să respire pe beat."
        )
    return "\n".join(blocks) + "\n"


def populate_demo_data(repo_root: Path) -> None:
    root = storage_root()
    artists_dir = root / "artists"
    artist_images_dir = root / "artist_images"
    tracks_dir = root / "tracks"
    ideas_dir = root / "ideas"

    for path in [artists_dir, artist_images_dir, tracks_dir, ideas_dir]:
        path.mkdir(parents=True, exist_ok=True)

    artists = [
        {
            "id": "a1b2c3d4e5f6",
            "name": "Lunetă Nord",
            "description": "Voce introspectivă, accent pe storytelling urban și hook-uri cinematice.",
            "image_colors": ((33, 55, 98), (205, 108, 66)),
            "tracks": [
                ("0a1b2c3d4e5f", "Bulevardul care tace", "aprins", "Berceni", "Ține-mă sus"),
                ("1a2b3c4d5e6f", "Neon pe zid", "tăios", "Titan", "Nu dau înapoi"),
            ],
        },
        {
            "id": "b1c2d3e4f5a6",
            "name": "Cadru Brut",
            "description": "Flow dens, imagini crude, construcție tehnică pe secțiuni lungi.",
            "image_colors": ((72, 34, 88), (222, 168, 77)),
            "tracks": [
                ("2a3b4c5d6e7f", "Oraș în compresie", "granit", "Drumul Taberei", "Ridic presiunea"),
                ("3a4b5c6d7e8f", "Pe frecvență joasă", "adânc", "Rahova", "Trec prin fum"),
            ],
        },
        {
            "id": "c1d2e3f4a5b6",
            "name": "Sablon Liber",
            "description": "Joc de cuvinte cu tranziții melodice, hook-uri memorabile și cadru live.",
            "image_colors": ((22, 92, 76), (231, 88, 120)),
            "tracks": [
                ("4a5b6c7d8e9f", "Trepte de fum", "elastic", "Pantelimon", "Țin firul viu"),
                ("5a6b7c8d9e0f", "Linii pe beton", "fierbinte", "Colentina", "Rămân pe fază"),
            ],
        },
    ]

    # clean old generated artists/tracks/ideas in our fixed id namespace
    known_ids = {artist["id"] for artist in artists}
    known_ids.update(track_id for artist in artists for track_id, *_ in artist["tracks"])
    known_ids.update({"d1e2f3a4b5c6", "d1e2f3a4b5c7", "d1e2f3a4b5c8"})

    for artist_file in artists_dir.glob("*.json"):
        if artist_file.stem in known_ids:
            artist_file.unlink(missing_ok=True)
    for folder in tracks_dir.iterdir():
        if folder.is_dir() and folder.name in known_ids:
            for child in sorted(folder.rglob("*"), reverse=True):
                if child.is_file():
                    child.unlink(missing_ok=True)
                elif child.is_dir():
                    child.rmdir()
            folder.rmdir()
    for folder in ideas_dir.iterdir():
        if folder.is_dir() and folder.name.startswith("d1e2f3a4b5c"):
            for child in sorted(folder.rglob("*"), reverse=True):
                if child.is_file():
                    child.unlink(missing_ok=True)
                elif child.is_dir():
                    child.rmdir()
            folder.rmdir()

    for artist in artists:
        artist_id = artist["id"]
        artist_image_path = artist_images_dir / f"{artist_id}.png"
        gradient_image(artist_image_path, 768, 768, artist["image_colors"][0], artist["image_colors"][1])

        write_json(
            artists_dir / f"{artist_id}.json",
            {
                "id": artist_id,
                "name": artist["name"],
                "description": artist["description"],
                "image": str(artist_image_path),
            },
        )

        for idx, (track_id, title, mood, district, hook_phrase) in enumerate(artist["tracks"], start=1):
            track_dir = tracks_dir / track_id
            lyrics_dir = track_dir / "lyrics"
            lyrics_dir.mkdir(parents=True, exist_ok=True)
            artwork_path = track_dir / "artwork.png"
            gradient_image(
                artwork_path,
                1024,
                1024,
                (40 + idx * 20, 40 + idx * 10, 80 + idx * 18),
                (220 - idx * 10, 120 + idx * 8, 70 + idx * 9),
            )

            final_text, raw_text = build_track_texts(title, mood, district, hook_phrase)
            (lyrics_dir / "final.txt").write_text(final_text, encoding="utf-8")
            (lyrics_dir / "raw.txt").write_text(raw_text, encoding="utf-8")

            used_material = used_material_entries(raw_text.splitlines())
            settings_payload = {
                "schema_version": 1,
                "id": track_id,
                "artist_id": artist_id,
                "name": title,
                "tempo": 78 + idx * 6,
                "length": "06:42",
                "working_directory": str(track_dir),
                "artwork": str(artwork_path),
                "casing_mode": "preserve",
                "used_material": used_material,
                "dismissed_material": [],
                "last_opened_unix": 1785690000 + idx,
            }
            write_json(track_dir / "settings.json", settings_payload)

    ideas = [
        {
            "id": "d1e2f3a4b5c6",
            "name": "Concept album - blocuri și ploaie",
            "in_out": idea_text("IN/OUT", 120),
            "verses": idea_text("VERSE DRAFT", 180),
            "hooks": idea_text("HOOK/BRIDGE", 140),
        },
        {
            "id": "d1e2f3a4b5c7",
            "name": "Pachet punchline - sesiune live",
            "in_out": idea_text("IN/OUT LIVE", 130),
            "verses": idea_text("VERSE LIVE", 170),
            "hooks": idea_text("HOOK LIVE", 150),
        },
        {
            "id": "d1e2f3a4b5c8",
            "name": "Scenă 3 - hook-uri contrast",
            "in_out": idea_text("IN/OUT SCENA", 125),
            "verses": idea_text("VERSE SCENA", 175),
            "hooks": idea_text("HOOK SCENA", 155),
        },
    ]

    for idx, idea in enumerate(ideas, start=1):
        idea_dir = ideas_dir / idea["id"]
        idea_dir.mkdir(parents=True, exist_ok=True)
        write_json(
            idea_dir / "settings.json",
            {
                "schema_version": 1,
                "id": idea["id"],
                "name": idea["name"],
                "created_unix": 1785691000 + idx,
                "updated_unix": 1785692000 + idx,
                "last_opened_unix": 1785693000 + idx,
            },
        )
        (idea_dir / "in_out.txt").write_text(idea["in_out"], encoding="utf-8")
        (idea_dir / "verses.txt").write_text(idea["verses"], encoding="utf-8")
        (idea_dir / "hooks_bridges.txt").write_text(idea["hooks"], encoding="utf-8")

    screenshots_dir = repo_root / "docs" / "screenshots"
    screenshot_palettes = [
        ((23, 24, 29), (49, 55, 63), (66, 72, 84)),
        ((20, 24, 28), (43, 59, 63), (73, 63, 87)),
        ((22, 22, 26), (61, 53, 70), (78, 70, 58)),
        ((17, 21, 25), (44, 51, 60), (62, 68, 79)),
        ((18, 23, 19), (49, 70, 57), (72, 78, 64)),
        ((21, 19, 24), (63, 49, 70), (83, 60, 77)),
        ((17, 24, 29), (45, 68, 78), (65, 70, 93)),
        ((24, 21, 18), (68, 57, 46), (92, 78, 60)),
    ]

    for index, palette in enumerate(screenshot_palettes, start=1):
        screenshot_mock(screenshots_dir / f"{index:02d}-workflow.png", palette)

    print("ROPER showcase data populated.")
    print(f"Storage root: {root}")
    print(f"Screenshots: {screenshots_dir}")


def main() -> None:
    repo_root = Path(__file__).resolve().parents[1]
    populate_demo_data(repo_root)


if __name__ == "__main__":
    main()
