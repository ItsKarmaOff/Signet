#!/usr/bin/env python3
"""Builds the CRX fixtures used by the unit tests (needs python3 and the openssl command).

    python3 make_fixtures.py

Writes rsa3.crx, ecdsa3.crx, rsa2.crx (signed CRX files of a tiny MV3 extension) and the id of each key in ids.json.
The files are checked in so tests need neither network nor openssl.
"""
import hashlib, io, json, os, struct, subprocess, tempfile, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))


def run(*args, data=None):
    return subprocess.run(args, input=data, capture_output=True, check=True).stdout


def varint(n):
    out = b""
    while True:
        b = n & 0x7F
        n >>= 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def field(num, payload):
    return varint(num << 3 | 2) + varint(len(payload)) + payload


def crx_id(pub_der):
    return "".join(chr(ord("a") + (b >> 4)) + chr(ord("a") + (b & 15)) for b in hashlib.sha256(pub_der).digest()[:16])


def make_zip():
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps({"manifest_version": 3, "name": "Fixture", "version": "1.0.0", "action": {"default_popup": "popup.html"}}))
        z.writestr("popup.html", "<!doctype html><title>popup</title>")
        z.writestr("_locales/en/messages.json", "{}")
    return buf.getvalue()


def keypair(kind, tmp):
    key = os.path.join(tmp, kind + ".pem")
    if kind == "rsa":
        run("openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", key)
    else:
        run("openssl", "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", key)
    pub = run("openssl", "pkey", "-in", key, "-pubout", "-outform", "DER")
    return key, pub


def sign(key, digest, data):
    return run("openssl", "dgst", "-" + digest, "-sign", key, data=data)


def crx3(kind, tmp, archive):
    key, pub = keypair(kind, tmp)
    cid = hashlib.sha256(pub).digest()[:16]
    signed_header = field(1, cid)
    msg = b"CRX3 SignedData\x00" + struct.pack("<I", len(signed_header)) + signed_header + archive
    proof = field(1, pub) + field(2, sign(key, "sha256", msg))
    header = field(2 if kind == "rsa" else 3, proof) + field(10000, signed_header)
    return b"Cr24" + struct.pack("<II", 3, len(header)) + header + archive, crx_id(pub)


def crx2(tmp, archive):
    key, pub = keypair("rsa", tmp)
    sig = sign(key, "sha1", archive)
    return b"Cr24" + struct.pack("<III", 2, len(pub), len(sig)) + pub + sig + archive, crx_id(pub)


def main():
    archive = make_zip()
    ids = {}
    with tempfile.TemporaryDirectory() as tmp:
        for name, (data, cid) in {
            "rsa3.crx": crx3("rsa", tmp, archive),
            "ecdsa3.crx": crx3("ec", tmp, archive),
            "rsa2.crx": crx2(tmp, archive),
        }.items():
            with open(os.path.join(HERE, name), "wb") as f:
                f.write(data)
            ids[name] = cid
    with open(os.path.join(HERE, "ids.json"), "w") as f:
        json.dump(ids, f, indent=2)
    print(ids)


main()
