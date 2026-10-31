"""Chinese mod names and descriptions, taken from the mods' own zh_cn lang files.

Mod Menu (Fabric/Quilt) reads `modmenu.nameTranslation.<id>` and
`modmenu.descriptionTranslation.<id>` / `modmenu.summaryTranslation.<id>`;
NeoForge reads `fml.menu.mods.info.displayname.<id>` and
`fml.menu.mods.info.description.<id>`. Mods that ship these in zh_cn carry
their own translation, so nothing is taken from third-party sites.

usage: python3 -I extract.py OUT.json [COUNT]
"""
import concurrent.futures as cf
import io
import json
import re
import sys
import time
import urllib.parse
import urllib.request
import zipfile

API = "https://api.modrinth.com/v2"
UA = {"User-Agent": "saudademjj/islet-launcher mod-name-extractor"}
CHINESE = re.compile(r"[㐀-䶿一-鿿]")
LANG = re.compile(r"^assets/[^/]+/lang/zh_cn\.json$", re.I)
NAME_KEYS = ("modmenu.nameTranslation.", "fml.menu.mods.info.displayname.")
DESC_KEYS = ("modmenu.summaryTranslation.", "modmenu.descriptionTranslation.", "fml.menu.mods.info.description.")
MAX_JAR = 40 * 1024 * 1024


def get(url, tries=4):
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
                return r.read(MAX_JAR + 1)
        except Exception:
            if attempt == tries - 1:
                raise
            time.sleep(2 + attempt * 3)


def top_projects(count):
    hits = []
    for offset in range(0, count, 100):
        q = urllib.parse.urlencode({"facets": '[["project_type:mod"]]', "index": "downloads", "limit": 100, "offset": offset})
        hits += json.loads(get(f"{API}/search?{q}"))["hits"]
    return hits[:count]


def versions(ids):
    out = {}
    for i in range(0, len(ids), 100):
        q = urllib.parse.urlencode({"ids": json.dumps(ids[i : i + 100])})
        for v in json.loads(get(f"{API}/versions?{q}")):
            out[v["id"]] = v
    return out


def mod_ids(jar):
    ids = set()
    for name in ("fabric.mod.json", "quilt.mod.json"):
        if name in jar.namelist():
            try:
                data = json.loads(jar.read(name).decode("utf-8", "replace"), strict=False)
                ids.add(data.get("id") or data.get("quilt_loader", {}).get("id"))
            except Exception:
                pass
    for name in ("META-INF/mods.toml", "META-INF/neoforge.mods.toml"):
        if name in jar.namelist():
            ids.update(re.findall(r'^\s*modId\s*=\s*"([^"]+)"', jar.read(name).decode("utf-8", "replace"), re.M))
    ids.discard(None)
    return ids


def clean(text):
    text = re.sub(r"§.", "", str(text)).replace("\\n", " ").replace("\n", " ")
    # Placeholder API / MiniMessage tags.
    text = re.sub(r"</?[a-z_#:.0-9 ]*>", "", text, flags=re.I)
    return re.sub(r"\s+", " ", text).strip()


def extract(hit, version):
    files = version.get("files") or []
    file = next((f for f in files if f.get("primary")), files[0] if files else None)
    if not file or file.get("size", 0) > MAX_JAR:
        return None
    jar = zipfile.ZipFile(io.BytesIO(get(file["url"])))
    ids = mod_ids(jar)
    names, descs = {}, {}
    for entry in jar.namelist():
        if not LANG.match(entry):
            continue
        try:
            lang = json.loads(jar.read(entry).decode("utf-8-sig", "replace"), strict=False)
        except Exception:
            continue
        for key, value in lang.items():
            if not isinstance(value, str):
                continue
            for prefix in NAME_KEYS:
                if key.startswith(prefix):
                    names[key[len(prefix) :]] = clean(value)
            for prefix in DESC_KEYS:
                # The summary is the short form; keep it over the long one.
                if key.startswith(prefix) and key[len(prefix) :] not in descs:
                    descs[key[len(prefix) :]] = clean(value)
    # The project's own mod id, else the only one translated.
    own = next((i for i in ids if i in names or i in descs), None) or (next(iter(names)) if len(names) == 1 else None)
    if not own:
        return None
    zh, desc = names.get(own), descs.get(own)
    if zh and (not CHINESE.search(zh) or zh.lower() == hit["title"].lower() or len(zh) > 40):
        zh = None
    if desc and (not CHINESE.search(desc) or len(desc) < 4):
        desc = None
    if desc and len(desc) > 120:
        desc = desc[:118].rstrip("，,。. ") + "…"
    if not zh and not desc:
        return None
    return {"slug": hit["slug"], "en": hit["title"], "zh": zh, "desc": desc, "license": hit.get("license"), "downloads": hit["downloads"]}


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 1500
    hits = top_projects(count)
    vs = versions([h["latest_version"] for h in hits if h.get("latest_version")])
    found, failed = [], 0
    with cf.ThreadPoolExecutor(8) as pool:
        jobs = {pool.submit(extract, h, vs[h["latest_version"]]): h for h in hits if h.get("latest_version") in vs}
        for n, job in enumerate(cf.as_completed(jobs), 1):
            try:
                item = job.result()
                if item:
                    found.append(item)
            except Exception as e:
                failed += 1
                print("fail", jobs[job]["slug"], e, file=sys.stderr)
            if n % 100 == 0:
                print(n, "checked,", len(found), "found", file=sys.stderr)
    found.sort(key=lambda i: -i["downloads"])
    json.dump(found, open(out, "w"), ensure_ascii=False, indent=1)
    print(len(hits), "projects,", len(found), "with Chinese,", failed, "failed", file=sys.stderr)


main()
