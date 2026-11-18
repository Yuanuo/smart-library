"""Generate the small dictionary samples under samples/dict.

These are synthetic but follow the real binary layouts: StarDict 2.4.2 and
MDict 2.0 with encrypted key block info (Encrypted="2"). They are used by the
Rust tests and for manual CLI checks; the real dictionaries stay outside the
repository.

    python tools/make_dict_samples.py
"""

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "samples" / "dict"


def rol(x, n):
    return ((x << n) | (x >> (32 - n))) & 0xFFFFFFFF


R = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5, 2, 14, 11, 8,
    3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12,
    1, 9, 11, 10, 0, 8, 12, 4, 13, 3, 7, 15, 14, 5, 6, 2,
]
RR = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12,
    6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12, 4, 9, 1, 2,
    15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13,
    8, 6, 4, 1, 3, 11, 15, 0, 5, 12, 2, 13, 9, 7, 10, 14,
]
S = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8,
    7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15, 9, 11, 7, 13, 12,
    11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5,
    11, 12, 14, 15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12,
]
SS = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6,
    9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12, 7, 6, 15, 13, 11,
    9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5,
    15, 5, 8, 11, 14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8,
]
K = [0x00000000, 0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC]
KK = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x00000000]


def f(j, x, y, z):
    if j == 0:
        return x ^ y ^ z
    if j == 1:
        return (x & y) | ((~x) & z)
    if j == 2:
        return (x | (~y)) ^ z
    return (x & z) | (y & (~z))


def ripemd128(data):
    h0, h1, h2, h3 = 0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476
    msg = bytearray(data)
    length = len(data) * 8
    msg.append(0x80)
    while len(msg) % 64 != 56:
        msg.append(0)
    msg += struct.pack("<Q", length)
    for off in range(0, len(msg), 64):
        X = list(struct.unpack("<16I", msg[off:off + 64]))
        a1, b1, c1, d1 = h0, h1, h2, h3
        a2, b2, c2, d2 = h0, h1, h2, h3
        for i in range(64):
            j = i >> 4
            t = rol((a1 + f(j, b1, c1, d1) + X[R[i]] + K[j]) & 0xFFFFFFFF, S[i])
            a1, b1, c1, d1 = d1, t, b1, c1
            t = rol((a2 + f(3 - j, b2, c2, d2) + X[RR[i]] + KK[j]) & 0xFFFFFFFF, SS[i])
            a2, b2, c2, d2 = d2, t, b2, c2
        t = (h1 + c1 + d2) & 0xFFFFFFFF
        h1 = (h2 + d1 + a2) & 0xFFFFFFFF
        h2 = (h3 + a1 + b2) & 0xFFFFFFFF
        h3 = (h0 + b1 + c2) & 0xFFFFFFFF
        h0 = t
    return struct.pack("<4I", h0, h1, h2, h3)


def swap_nibbles(b):
    return ((b >> 4) | (b << 4)) & 0xFF


def fast_decrypt(data, key):
    out = bytearray()
    previous = 0x36
    for i, b in enumerate(data):
        out.append(swap_nibbles(b) ^ previous ^ (i & 0xFF) ^ key[i % len(key)])
        previous = b
    return bytes(out)


def fast_encrypt(data, key):
    out = bytearray()
    previous = 0x36
    for i, b in enumerate(data):
        c = swap_nibbles(b ^ previous ^ (i & 0xFF) ^ key[i % len(key)])
        out.append(c)
        previous = c
    return bytes(out)


def zlib_block(raw):
    packed = zlib.compress(raw)
    adler = zlib.adler32(raw) & 0xFFFFFFFF
    return struct.pack("<I", 2) + adler.to_bytes(4, "big") + packed


def mdict_key_info(blocks):
    raw = bytearray()
    for head, tail, num_entries, comp_size, decomp_size in blocks:
        raw += struct.pack(">Q", num_entries)
        hb = head.encode("utf-8")
        tb = tail.encode("utf-8")
        raw += struct.pack(">H", len(hb)) + hb + b"\0"
        raw += struct.pack(">H", len(tb)) + tb + b"\0"
        raw += struct.pack(">QQ", comp_size, decomp_size)
    block = zlib_block(bytes(raw))
    key = ripemd128(block[4:8] + struct.pack("<L", 0x3695))
    return block[:8] + fast_encrypt(block[8:], key), len(raw)


def build_mdict(path, title, entries, encoding="UTF-8"):
    """entries: list of (key, record bytes). One key block, one record block."""
    keys = [k for k, _ in entries]
    records = [r for _, r in entries]
    key_raw = bytearray()
    offset = 0
    offsets = []
    for key, record in zip(keys, records):
        offsets.append(offset)
        offset += len(record)
        key_raw += struct.pack(">Q", offsets[-1])
        key_raw += key.encode(encoding) + b"\0"
    key_block = zlib_block(bytes(key_raw))
    record_raw = b"".join(records)
    record_block = zlib_block(record_raw)
    key_info, key_info_decomp = mdict_key_info(
        [(keys[0], keys[-1], len(keys), len(key_block), len(key_raw))]
    )
    header = (
        '<Dictionary GeneratedByEngineVersion="2.0" RequiredEngineVersion="2.0" '
        'Format="Html" KeyCaseSensitive="No" StripKey="Yes" Encrypted="2" '
        f'Encoding="{encoding}" Title="{title}"/>'
    ).encode("utf-16-le")
    record_header = struct.pack(
        ">QQQQ", 1, len(records), 16, len(record_block)
    ) + struct.pack(">QQ", len(record_block), len(record_raw))
    out = (
        struct.pack(">I", len(header))
        + header
        + (zlib.adler32(header) & 0xFFFFFFFF).to_bytes(4, "little")
        + struct.pack(">QQQQQ", 1, len(records), key_info_decomp, len(key_info), len(key_block))
        + (0).to_bytes(4, "big")
        + key_info
        + key_block
        + record_header
        + record_block
    )
    path.write_bytes(out)


PNG_1PX = bytes.fromhex(
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
    "0000000d49444154789c6360000000020001e221bc330000000049454e44ae426082"
)


def main():
    txt = OUT / "txt"
    txt.mkdir(parents=True, exist_ok=True)
    (txt / "佛学小词典.txt").write_text(
        "【般若】梵语 prajñā 的音译，意为智慧。\\\\n又译作「波若」。\n"
        "【菩提】觉、道。指断除烦恼、成就佛果的智慧。\n"
        "【四谛】苦、集、灭、道，佛教的基本教义。\n"
        "这一行没有方括号，应被忽略。\n",
        encoding="utf-8",
        newline="\n",
    )

    stardict = OUT / "stardict"
    stardict.mkdir(parents=True, exist_ok=True)
    words = [("三法印", "诸行无常、诸法无我、涅槃寂静。"), ("佛", "觉者。"), ("般若", "智慧。")]
    idx = bytearray()
    dic = bytearray()
    for word, text in words:
        data = text.encode("utf-8")
        idx += word.encode("utf-8") + b"\0" + struct.pack(">II", len(dic), len(data))
        dic += data
    (stardict / "测试星译.idx").write_bytes(idx)
    (stardict / "测试星译.dict").write_bytes(dic)
    (stardict / "测试星译.ifo").write_text(
        "StarDict's dict ifo file\n"
        "version=2.4.2\n"
        f"wordcount={len(words)}\n"
        f"idxfilesize={len(idx)}\n"
        "bookname=测试星译词典\n"
        "sametypesequence=m\n",
        encoding="utf-8",
        newline="\n",
    )

    mdx_dir = OUT / "mdx"
    mdx_dir.mkdir(parents=True, exist_ok=True)
    mdx_entries = [
        ("般若", '<p><b>般若</b>：梵语 prajñā 的音译，意为智慧。<img src="images/lotus.png"></p>'),
        ("菩提", "<p><b>菩提</b>：觉、道。指断除烦恼、成就佛果的智慧。</p>"),
        ("四谛", "<p><b>四谛</b>：苦、集、灭、道。</p>"),
        ("智慧", "@@@LINK=般若"),
    ]
    build_mdict(mdx_dir / "测试词典.mdx", "测试词典", [(k, v.encode("utf-8")) for k, v in mdx_entries])
    build_mdict(
        mdx_dir / "测试词典.mdd",
        "测试词典图片",
        [("\\images\\lotus.png", PNG_1PX)],
    )


if __name__ == "__main__":
    main()
