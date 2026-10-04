"""Build the committed Ren'Py archive fixtures. Run from this directory:

    py -3 make_fixtures.py

The Rust tests read the files written here and do not need Python.
"""

import pickle
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent
KEY = 0x11223344


def write_v3(name, index, payload, key=KEY):
    body = pickle.dumps(index, protocol=2)
    compressed = zlib.compress(body)
    header = f"RPA-3.0 {34 + len(payload):016x} {key:08x}\n".encode()
    assert len(header) == 34
    (OUT / name).write_bytes(header + payload + compressed)


def write_v2(name, index, payload):
    compressed = zlib.compress(pickle.dumps(index, protocol=2))
    offset = 25 + len(payload)  # "RPA-2.0 " + 16 hex + newline, then payload
    header = f"RPA-2.0 {offset:016x}\n".encode()
    (OUT / name).write_bytes(header + payload + compressed)


def main():
    payload = b"HELLO" + b"NOTE"
    # offsets: header is 34 bytes, HELLO at 34, NOTE at 39
    key = KEY
    index = {
        "dir/script.rpy": [(34 ^ key, 5 ^ key)],
        "note.txt": [(39 ^ key, 4 ^ key, b"PRE:")],
        "multi.txt": [(34 ^ key, 5 ^ key), (39 ^ key, 4 ^ key)],
    }
    write_v3("v3-proto2.rpa", index, payload)

    for proto in (0, 2, 4, 5):
        try:
            blob = pickle.dumps(index, protocol=proto)
        except Exception as exc:
            print(f"protocol {proto} skipped: {exc}")
            continue
        (OUT / f"index-proto{proto}.pickle").write_bytes(blob)
        print(f"index-proto{proto}.pickle {len(blob)} bytes")

    # Python 3 protocol 2 stores bytes keys through _codecs.encode.
    bytes_key = {b"script.rpy": [(34 ^ key, 5 ^ key)]}
    (OUT / "index-bytes-key.pickle").write_bytes(pickle.dumps(bytes_key, protocol=2))

    # RPA-2.0, no key. Same payload placed after a 25-byte header, so HELLO is at 25.
    index2 = {
        "dir/script.rpy": [(25, 5)],
        "note.txt": [(30, 4, "PRE:")],
    }
    write_v2("v2.rpa", index2, payload)

    # RPA-1.0: the .rpi is the zlib index, the .rpa is raw bytes (no header).
    index1 = {"dir/script.rpy": [(0, 5)], "note.txt": [(5, 4)]}
    (OUT / "v1.rpi").write_bytes(zlib.compress(pickle.dumps(index1, protocol=2)))
    (OUT / "v1.rpa").write_bytes(payload)

    # A header this reader must refuse, naming the bytes it saw.
    (OUT / "custom.rpa").write_bytes(b"RPA-3.2 not-a-real-format\n")

    # An entry whose name would escape the extract folder.
    bad = {"../evil.txt": [(34 ^ key, 5 ^ key)]}
    write_v3("escape.rpa", bad, b"HELLO")

    print("wrote fixtures to", OUT)


if __name__ == "__main__":
    main()
