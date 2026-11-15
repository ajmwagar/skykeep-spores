#!/usr/bin/env python3
"""Content ids for a Spore's listing artwork.

A listing points at a CID, never a path (`soil-spore` refuses anything else in
`catalog.screenshots[].cid`, `catalog.record` and `presentation.icon`). Those
CIDs are derived from bytes, so they belong in a script rather than in a
person's memory: change a screenshot, re-run this, paste the new value.

The construction is the one `soil_spore::cid_v1_raw` makes — raw codec (0x55)
over sha2-256 (0x12, 32 bytes), multibase base32 (`b`) — and base32 itself comes
from Python's reference encoder rather than a hand-rolled alphabet.

    python3 tools/spore-cid.py docs/media/spore-contacts.png
    python3 tools/spore-cid.py --check

The second form reads every `ipfs://` cited by a package manifest and fails when
one cannot be reproduced from a file in this repository: a dead reference is a
listing nobody can verify.
"""

import base64
import hashlib
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def cid_v1_raw(data: bytes) -> str:
    digest = hashlib.sha256(data).digest()
    block = b"\x01\x55\x12\x20" + digest
    return "b" + base64.b32encode(block).decode("ascii").lower().rstrip("=")


def cid_of(path: pathlib.Path) -> str:
    return cid_v1_raw(path.read_bytes())


def artwork() -> dict:
    """Every file a listing may legitimately cite, by CID.

    `docs/media` holds the screenshots and each package ships its own mark under
    `assets/`. Nothing else is artwork, so nothing else may be cited.
    """
    found = {}
    paths = list((ROOT / "docs" / "media").glob("*.png"))
    for package in (ROOT / "spores").iterdir():
        if package.is_dir():
            paths.extend((package / "assets").glob("*"))
    for path in paths:
        if path.is_file():
            found[cid_of(path)] = path.relative_to(ROOT)
    return found


def check() -> int:
    known = artwork()
    failed = 0
    for manifest in sorted((ROOT / "spores").glob("*/spore.toml")):
        text = manifest.read_text()
        for cid in re.findall(r"ipfs://(b[a-z2-7]+)", text):
            where = manifest.parent.name
            if cid in known:
                print(f"ok   {where}: {known[cid]}")
            else:
                print(f"DEAD {where}: ipfs://{cid} is not the content id of any artwork")
                failed += 1
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if argv and argv[0] == "--check":
        return check()
    if not argv:
        print(__doc__)
        return 2
    for name in argv:
        path = pathlib.Path(name)
        cid = cid_of(path)
        print(f"ipfs://{cid}\t{path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
