#!/usr/bin/env python3
"""Merges two runs of the Go fixture recorder into the golden fixtures.

    NINJACAT_FIXTURE_DIR=/tmp/a go test -count=1 ./intake
    NINJACAT_FIXTURE_DIR=/tmp/b go test -count=1 ./intake
    ./merge.py /tmp/a /tmp/b go/

Whatever differs between the two runs (receive times, minted ids, multipart
boundaries) is listed under "volatile", and the replay does not compare it.
A JSON response body is also stored parsed, as "body_json", so a volatile id
inside it does not make the whole body volatile.
"""
import base64, glob, json, os, sys

def body_json(message):
    raw = base64.b64decode(message["body_b64"])
    if not raw.strip():
        return None
    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return None

def diff(a, b, path, out):
    if type(a) is not type(b):
        out.append(path)
    elif isinstance(a, dict):
        for key in sorted(set(a) | set(b)):
            if key not in a or key not in b:
                out.append(f"{path}/{key}")
            else:
                diff(a[key], b[key], f"{path}/{key}", out)
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append(path)
        else:
            for i, (x, y) in enumerate(zip(a, b)):
                diff(x, y, f"{path}/{i}", out)
    elif a != b:
        out.append(path)

def main(first, second, target):
    count = 0
    for path in sorted(glob.glob(os.path.join(first, "*", "*.json"))):
        relative = os.path.relpath(path, first)
        a = json.load(open(path))
        b = json.load(open(os.path.join(second, relative)))

        for fixture in (a, b):
            parsed = body_json(fixture["response"])
            if parsed is not None:
                fixture["response"]["body_json"] = parsed

        volatile = []
        diff(a["response"], b["response"], "/response", volatile)
        diff(a["sends"], b["sends"], "/sends", volatile)
        if "body_json" in a["response"]:
            volatile = [v for v in volatile if v != "/response/body_b64"]
        a["volatile"] = volatile

        out = os.path.join(target, relative)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        with open(out, "w") as f:
            json.dump(a, f, indent=1, sort_keys=True, ensure_ascii=False)
            f.write("\n")
        count += 1
    print(f"{count} fixtures")

if __name__ == "__main__":
    main(*sys.argv[1:4])
