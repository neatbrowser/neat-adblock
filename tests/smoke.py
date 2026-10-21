#!/usr/bin/env python3
"""Loads the built library through ctypes and exercises it against a real neat-filters release.

usage: python3 tests/smoke.py <path to libneat_adblock.so|neat_adblock.dll> <neat-filters dist dir>
"""
import ctypes as C
import json
import sys
import threading
import time

lib = C.CDLL(sys.argv[1])
dist = sys.argv[2]


class Result(C.Structure):
    _fields_ = [("action", C.c_int32), ("redirect", C.c_void_p), ("rewritten_url", C.c_void_p)]


P = C.c_void_p
lib.neat_abi_version.restype = C.c_uint32
lib.neat_engine_create.restype = P
lib.neat_engine_create.argtypes = [C.c_char_p, C.POINTER(P)]
lib.neat_engine_free.argtypes = [P]
lib.neat_string_free.argtypes = [P]
lib.neat_check_request.restype = C.c_int32
lib.neat_check_request.argtypes = [P, C.c_char_p, C.c_char_p, C.c_char_p, C.c_char_p, C.POINTER(Result)]
lib.neat_check_result_free.argtypes = [C.POINTER(Result)]
lib.neat_check_popup.restype = C.c_int32
lib.neat_check_popup.argtypes = [P, C.c_char_p, C.c_char_p]
lib.neat_get_csp.restype = C.c_int32
lib.neat_get_csp.argtypes = [P, C.c_char_p, C.c_char_p, C.c_char_p, C.POINTER(P)]
lib.neat_cosmetic_resources.restype = C.c_int32
lib.neat_cosmetic_resources.argtypes = [P, C.c_char_p, C.POINTER(P)]
lib.neat_hidden_class_id_selectors.restype = C.c_int32
lib.neat_hidden_class_id_selectors.argtypes = [P, C.c_char_p, C.c_char_p, C.c_char_p, C.POINTER(P)]

failures = []


def check(name, cond):
    print(("ok   " if cond else "FAIL ") + name)
    if not cond:
        failures.append(name)


def take_string(p):
    s = C.string_at(p).decode() if p else None
    if p:
        lib.neat_string_free(p)
    return s


def request(e, url, source, ty):
    r = Result()
    rc = lib.neat_check_request(e, url.encode(), source.encode(), ty.encode(), None, C.byref(r))
    action = r.action
    redirect = C.string_at(r.redirect).decode() if r.redirect else None
    lib.neat_check_result_free(C.byref(r))
    check_freed = r.redirect is None and r.rewritten_url is None
    return rc, action, redirect, check_freed


check("abi version is 1", lib.neat_abi_version() == 1)

# Failure path
err = P()
e = lib.neat_engine_create(b"/nonexistent-dir", C.byref(err))
msg = take_string(err.value)
check("create with a bad directory returns NULL and an error", not e and msg and "manifest.json" in msg)
check("create with NULL directory fails cleanly", not lib.neat_engine_create(None, None))

# Real engine
t0 = time.time()
err = P()
engine = lib.neat_engine_create(dist.encode(), C.byref(err))
check(f"create from the release succeeds ({time.time() - t0:.1f}s)", bool(engine))
if not engine:
    print(take_string(err.value))
    sys.exit(1)

rc, action, redirect, freed = request(engine, "https://securepubads.g.doubleclick.net/tag/js/gpt.js", "https://example.com/", "script")
check("doubleclick gpt.js is blocked or redirected", rc == 0 and action in (1, 2))
if action == 2:
    check("redirect carries a data: URL", redirect is not None and redirect.startswith("data:"))
check("check_result_free clears the strings", freed)

rc, action, _, _ = request(engine, "https://example.com/", "https://example.com/", "document")
check("example.com document is allowed", rc == 0 and action == 0)

rc, _, _, _ = request(engine, "not a url", "https://example.com/", "script")
check("bad URL gives NEAT_ERR_URL", rc == -2)
r = Result()
check("null engine gives NEAT_ERR_ARGS", lib.neat_check_request(None, b"https://a.com/", b"https://b.com/", b"script", None, C.byref(r)) == -1)
check("null result pointer gives NEAT_ERR_ARGS", lib.neat_check_request(engine, b"https://a.com/", b"https://b.com/", b"script", None, None) == -1)

# Popups: take a real rule from the release
popups = [l for l in open(f"{dist}/popups.txt", encoding="utf-8").read().split("\n") if l.startswith("||") and l.endswith("^$document")]
check("popups.txt has simple host rules", len(popups) > 0)
host = popups[0][2:].split("^")[0]
check(f"popup to {host} is blocked", lib.neat_check_popup(engine, f"https://{host}/x".encode(), b"https://news.test/") == 1)
check("popup to an unrelated host is allowed", lib.neat_check_popup(engine, b"https://example.com/", b"https://news.test/") == 0)
check("popup with a bad URL gives NEAT_ERR_URL", lib.neat_check_popup(engine, b"nope", b"https://news.test/") == -2)

# Cosmetic
out = P()
rc = lib.neat_cosmetic_resources(engine, b"https://www.youtube.com/watch?v=x", C.byref(out))
cos = json.loads(take_string(out.value)) if rc == 0 else {}
check("cosmetic resources parse as JSON with the expected keys",
      rc == 0 and set(cos) == {"hide_selectors", "procedural_actions", "exceptions", "injected_script", "generichide"})
check("youtube.com gets an injected scriptlet", len(cos.get("injected_script", "")) > 1000)

out = P()
rc = lib.neat_hidden_class_id_selectors(engine, b'["ad","banner"]', b'["ad"]', json.dumps(cos.get("exceptions", [])).encode(), C.byref(out))
sel = json.loads(take_string(out.value)) if rc == 0 else None
check("generic hide selectors come back as a JSON array", rc == 0 and isinstance(sel, list))
out = P()
check("invalid JSON gives NEAT_ERR_ARGS", lib.neat_hidden_class_id_selectors(engine, b"nope", b"[]", b"[]", C.byref(out)) == -1)

out = P()
rc = lib.neat_get_csp(engine, b"https://example.com/", b"https://example.com/", b"document", C.byref(out))
take_string(out.value)
check("get_csp returns 0 or 1", rc in (0, 1))

# Concurrency: many threads on one engine
urls = [
    ("https://securepubads.g.doubleclick.net/tag/js/gpt.js", "script"),
    ("https://example.com/app.js", "script"),
    ("https://www.google-analytics.com/analytics.js", "script"),
    ("https://example.com/", "document"),
]
errors = []
N, THREADS = 3000, 8


def worker():
    for i in range(N):
        u, ty = urls[i % len(urls)]
        rc, action, _, _ = request(engine, u, "https://example.com/", ty)
        if rc != 0 or (i % len(urls) == 0 and action == 0):
            errors.append((u, rc, action))


t0 = time.time()
ts = [threading.Thread(target=worker) for _ in range(THREADS)]
[t.start() for t in ts]
[t.join() for t in ts]
dt = time.time() - t0
check(f"{THREADS} threads x {N} checks agree, no errors ({THREADS * N / dt:,.0f} checks/s incl. ctypes overhead)", not errors)

lib.neat_engine_free(engine)
lib.neat_engine_free(None)
check("freeing NULL is a no-op", True)

print("\nFAILED:" if failures else "\nall passed", failures or "")
sys.exit(1 if failures else 0)
