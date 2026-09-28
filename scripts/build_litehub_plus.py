"""Build a personal LiteHubPlus from a freshly generated LiteHub and local JCY.

No downloads or controller execution. Requires the development CASC exporter and
Pillow. Outputs a fresh directory, dependency audit, and a complete hash manifest.
JCY assets are local inputs, not bundled with this script or licensed for release.
"""
import argparse
import copy
import csv
import hashlib
import io
import json
import pathlib
import re
import shutil
import struct
import subprocess
import uuid

from PIL import Image, ImageDraw


REVISION = 1
CODES = [f"r{i:02}" for i in range(26, 34)] + ["pk1", "pk2", "pk3", "dhn", "bey", "mbr"]
BUFFS = ["battleorders", "battlecommand", "shout"]
LAYOUT = "data/global/ui/layouts/"
PATH_RE = re.compile(rb"data[/\\][A-Za-z0-9_.$/\\ -]+?\.(?:json|texture|particles|model|skeleton|animation|animations|physics|variantdata|objecteffects|timelines|sprite)(?![A-Za-z])", re.I)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def resource(path):
    path = path.replace("\\", "/").lower()
    if not path.startswith("data/") or any(p in ("", ".", "..") for p in path.split("/")) or ":" in path:
        raise ValueError(f"Unsafe resource: {path}")
    return path


def references(value):
    if isinstance(value, bytes):
        return {resource(v.decode("ascii")) for v in PATH_RE.findall(value)}
    if isinstance(value, str):
        return {resource(value)} if value.lower().startswith(("data/", "data\\")) else set()
    if isinstance(value, dict):
        return set().union(*(references(v) for v in value.values())) if value else set()
    if isinstance(value, list):
        return set().union(*(references(v) for v in value)) if value else set()
    return set()


def table(data):
    reader = csv.DictReader(io.StringIO(data.decode("utf-8-sig")), delimiter="\t")
    return reader.fieldnames, list(reader)


def table_bytes(columns, rows):
    out = io.StringIO(newline="")
    writer = csv.DictWriter(out, fieldnames=columns, delimiter="\t", lineterminator="\r\n")
    writer.writeheader()
    writer.writerows(rows)
    return out.getvalue().encode("utf-8")


class Builder:
    def __init__(self, args):
        self.args = args
        self.base = args.baseline.resolve()
        self.jcy = args.jcy.resolve()
        self.work = args.work.resolve()
        self.cache = self.work / "native"
        self.destination = args.output.resolve() / "LiteHubPlus"
        if self.destination.exists():
            raise ValueError(f"Refusing to replace existing Mod: {self.destination}")
        if not (self.jcy / "modinfo.json").is_file():
            raise ValueError("JCY input must be its .mpq directory")
        self.stage = args.output.resolve() / f".litehubplus-building-{uuid.uuid4()}"
        self.mpq = self.stage / "LiteHubPlus.mpq"
        self.sources = {}
        self.modified = set()
        self.dependency_roots = set()
        self.checked = set()
        self.missing = set()
        self.features = {}

    def export(self, paths):
        wanted = sorted({resource(p) for p in paths} - self.checked)
        if not wanted:
            return
        self.work.mkdir(parents=True, exist_ok=True)
        requests = self.work / "export-requests.json"
        write_json(requests, wanted)
        subprocess.run([str(self.args.exporter), str(self.args.game), str(self.cache), str(requests)], check=True)
        results = read_json(self.cache / "export-results.json")
        self.checked.update(wanted)
        self.missing.update(r["path"] for r in results if not r["available"])

    def native_bytes(self, path):
        path = resource(path)
        self.export([path])
        if path in self.missing:
            raise ValueError(f"Missing native resource: {path}")
        data = (self.cache / path).read_bytes()
        self.sources["native:" + path] = digest(data)
        return data

    def native_json(self, path):
        self.native_bytes(path)
        return read_json((self.cache / path).with_suffix(".parsed.json"))

    def jcy_bytes(self, path):
        path = resource(path)
        source = self.jcy / path
        if not source.is_file() and source.with_name(source.name + ".tmp").is_file():
            source = source.with_name(source.name + ".tmp")
        data = source.read_bytes()
        if not data:
            raise ValueError(f"JCY resource disabled: {source}")
        self.sources["jcy:" + source.relative_to(self.jcy).as_posix()] = digest(data)
        return data

    def jcy_json(self, path):
        return json.loads(self.jcy_bytes(path).decode("utf-8-sig"))

    def put(self, path, data):
        path = resource(path)
        target = self.mpq / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        self.modified.add(path)

    def put_json(self, path, value):
        self.put(path, (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))

    def current_json(self, path):
        p = self.mpq / path
        return read_json(p) if p.is_file() and p.stat().st_size else self.native_json(path)

    def attach(self, path, nodes, label):
        obj = self.current_json(path)
        existing = obj.setdefault("entities", [])
        used = {e.get("id") for e in existing}
        for i, node in enumerate(copy.deepcopy(nodes)):
            node["name"] = f"litehubplus_{label}_{i}"
            node["id"] = 1800000000 + i
            while node["id"] in used:
                node["id"] += 1
            used.add(node["id"])
            existing.append(node)
            self.dependency_roots.update(references(node))
        self.put_json(path, obj)

    def markers(self):
        roomfiles = sorted((self.jcy / "data/hd/roomtiles").glob("*.json"))
        self.export([p.relative_to(self.jcy).as_posix() for p in roomfiles])
        template = self.jcy_json("data/hd/roomtiles/act_1_cave_down.json")
        template = next(e for e in template["entities"] if e.get("name") == "jcy_entity_pointer")
        covered = []
        for p in roomfiles:
            rel = p.relative_to(self.jcy).as_posix().lower()
            if rel in self.missing:
                continue
            reference = self.jcy_json(rel)
            nodes = [e for e in reference.get("entities", []) if e.get("name") == "jcy_entity_pointer"]
            if not nodes and "_up" in p.stem:
                node = copy.deepcopy(template)
                for c in node["components"]:
                    if "prefab" in c:
                        c["prefab"] = "data/hd/env/porory/beacon/pf_beacon_upstairs.json"
                nodes = [node]
            if nodes:
                self.attach(rel, nodes, "entrance")
                covered.append(rel)
        self.features["map_markers"] = covered
        routes = [f"data/hd/env/preset/act1/crypt/crypt{d}warpprev.json" for d in "ensw"]
        routes += [f"data/hd/env/preset/act3/travincal/meph{d}warp.json" for d in "ensw"]
        self.export(routes)
        for rel in routes:
            nodes = [e for e in self.jcy_json(rel)["entities"] if e.get("name") == "jcy_entity_pointer"]
            if not nodes:
                raise ValueError(f"JCY direction reference missing: {rel}")
            self.attach(rel, nodes, "route")
        self.features["route_hints"] = {"presets": routes, "meaning": "recommended exploration direction; not live exit location"}

    def loot(self):
        mapping = self.native_json("data/hd/items/items.json")
        index = {code: value for row in mapping for code, value in row.items()}
        source_paths = ["data/hd/items/misc/" + index[c]["asset"] + ".json" for c in CODES]
        sprites = [f"data/hd/global/ui/items/misc/{index[c]['asset']}{suffix}" for c in CODES for suffix in (".sprite", ".lowend.sprite")]
        self.export(source_paths + sprites)
        reference = self.jcy_json("data/hd/items/misc/rune/vex_rune.json")
        beam = next(e for e in reference["entities"] if any(c.get("name") == "vfx_drop_light" for c in e.get("components", [])))
        for code in CODES:
            original = index[code]["asset"]
            source = "data/hd/items/misc/" + original + ".json"
            # Runes retain their conventional paths for the existing telemetry processor.
            asset = original if code.startswith("r") else f"litehubplus/{code}"
            target = f"data/hd/items/misc/{asset}.json"
            obj = self.native_json(source)
            self.put_json(target, obj)
            self.dependency_roots.update(references(obj))
            self.attach(target, [beam], "loot_beam")
            for suffix in (".sprite", ".lowend.sprite"):
                self.put(f"data/hd/global/ui/items/misc/{asset}{suffix}", self.native_bytes(f"data/hd/global/ui/items/misc/{original}{suffix}"))
            index[code]["asset"] = asset
        self.put_json("data/hd/items/items.json", mapping)
        self.features["loot_beams"] = {"codes": CODES, "quality_filter": False}

    def buffs(self):
        self.export(["data/global/excel/states.txt", "data/global/excel/sounds.txt"])
        sc, states = table(self.native_bytes("data/global/excel/states.txt"))
        ac, sounds = table(self.native_bytes("data/global/excel/sounds.txt"))
        _, reference = table(self.jcy_bytes("data/global/excel/sounds.txt"))
        refs = {r["Sound"]: r for r in reference}
        max_index = max(int(r["*Index"]) for r in sounds if r.get("*Index", "").isdigit())
        for i, buff in enumerate(BUFFS, 1):
            name = "litehubplus_" + buff + "_off"
            source = refs[buff + "_off"]
            row = {c: source.get(c, "") for c in ac}
            row.update({"Sound": name, "*Index": str(max_index+i), "FileName": f"litehubplus\\{buff}_off.flac"})
            sounds.append(row)
            target = next(r for r in states if r["state"] == buff)
            target["offsound"] = name
            self.put(f"data/hd/global/sfx/litehubplus/{buff}_off.flac", self.jcy_bytes(f"data/hd/global/sfx/skill/{buff}_off.flac"))
        self.put("data/global/excel/states.txt", table_bytes(sc, states))
        self.put("data/global/excel/sounds.txt", table_bytes(ac, sounds))
        self.features["buff_end_audio"] = {"states": BUFFS, "trigger": "states.offsound", "source": "local JCY voice", "runtime_verified": False}

    def grid_sprite(self, columns, rows, split_x, split_y, stem):
        # New code-generated UI geometry, not a replacement for item coordinates.
        cell = 98
        w, h = columns*cell, rows*cell
        image = Image.new("RGBA", (w, h), (12, 14, 17, 185))
        draw = ImageDraw.Draw(image)
        for y in range(rows):
            for x in range(columns):
                warm = x < split_x
                color = (88, 73, 44, 45) if warm else (40, 76, 87, 42)
                draw.rectangle((x*cell+2,y*cell+2,(x+1)*cell-3,(y+1)*cell-3), fill=color, outline=(122,113,91,145), width=2)
        draw.line((split_x*cell,0,split_x*cell,h-1), fill=(185,159,99,225), width=4)
        if split_y:
            draw.line((0,split_y*cell,w-1,split_y*cell), fill=(185,159,99,225), width=4)
        draw.rectangle((0,0,w-1,h-1), outline=(167,143,95,225), width=3)
        for lowend in (False, True):
            im = image.resize((w//2,h//2), Image.Resampling.LANCZOS) if lowend else image
            header = bytearray(40)
            header[:4] = b"SpA1"
            struct.pack_into("<H", header, 4, 31)
            struct.pack_into("<H", header, 6, im.width)
            for offset, value in [(8,im.width),(12,im.height),(20,1),(32,im.width*im.height*4),(36,4)]:
                struct.pack_into("<I", header, offset, value)
            self.put(f"data/hd/global/ui/litehubplus/{stem}{'.lowend' if lowend else ''}.sprite", bytes(header)+im.tobytes())
        image.save(self.work / f"{stem}-preview.png")
        return image

    def ui(self):
        paths = [LAYOUT+n+".json" for n in ("playerinventoryoriginallayouthd", "playerinventoryexpansionlayouthd", "bankoriginallayouthd", "bankexpansionlayouthd")]
        self.export(paths)
        inv = self.native_json(paths[1])
        bank = self.native_json(paths[3])
        self.grid_sprite(10,4,6,0,"inventory_grid")
        self.grid_sprite(10,10,5,5,"stash_grid")

        def backdrop(name, x, y):
            return {"type":"ImageWidget", "name":"LiteHubPlusGrid", "fields":{"rect":{"x":x,"y":y},"filename":f"litehubplus\\{name}"}}

        inv_grid = next(c for c in self.native_json(paths[0])["children"] if c["name"] == "grid")
        assert inv_grid["fields"]["cellCount"] == {"x":10,"y":4}
        rect = inv_grid["fields"]["rect"]
        idx = next(i for i,c in enumerate(inv["children"]) if c["name"] == "grid")
        inv["children"].insert(idx,backdrop("inventory_grid",rect["x"],rect["y"]))
        container = next(c for c in bank["children"] if c["name"] == "basicstash_container")
        grid = next(c for c in container["children"] if c["name"] == "grid")
        assert grid["fields"]["cellCount"] == {"x":10,"y":10}
        rect = grid["fields"]["rect"]
        container["children"].insert(0,backdrop("stash_grid",rect["x"],rect["y"]))
        # Restore the base layout and equipment-slot sprites hidden by main.
        for path, obj in [(paths[0],self.native_json(paths[0])),(paths[1],inv),(paths[2],self.native_json(paths[2])),(paths[3],bank)]:
            self.put_json(path,obj)
        sprite_paths = []
        for p in self.mpq.glob("data/hd/global/ui/panel/inventory/*.sprite"):
            if p.stat().st_size == 0:
                sprite_paths.append(p.relative_to(self.mpq).as_posix())
        self.export(sprite_paths)
        for p in sprite_paths:
            self.put(p,self.native_bytes(p))
        self.features["inventory_layout"] = {"grid":"10x4", "reference_zones":["6 columns charms","4 columns loot"],"capacity_changed":False}
        self.features["stash_layout"] = {"grid":"10x10", "reference_zones":"four 5x5 quadrants", "native_tabs_preserved":True}

    def dependencies(self):
        pending = set(self.dependency_roots)
        visited = set()
        audit = []
        while pending:
            batch = sorted(pending - visited)
            if not batch:
                break
            pending = set()
            self.export(batch)
            # D2R resolves logical .model references to compiled _lod0.model files.
            model_aliases = {p: p[:-6]+"_lod0.model" for p in batch if p in self.missing and p.endswith(".model")}
            self.export(model_aliases.values())
            for path in batch:
                visited.add(path)
                logical_path = path
                if path in self.missing and path.endswith(".texture"):
                    split = [path[:-8]+part+".texture" for part in ("$rgb","$a")]
                    if all((self.jcy/p).is_file() for p in split):
                        for part in split:
                            self.put(part,self.jcy_bytes(part))
                        audit.append({"path":path,"origin":"jcy_split_texture","resolved_paths":split})
                        continue
                if path in self.missing and path.endswith(".animation"):
                    bundles = [str(pathlib.PurePosixPath(path).parent/("combined."+ext)) for ext in ("animations","timelines")]
                    self.export(bundles)
                    for bundle in bundles:
                        self.put(bundle,self.native_bytes(bundle))
                    audit.append({"path":path,"origin":"native_animation_bundle","resolved_paths":bundles})
                    continue
                if path in model_aliases and model_aliases[path] not in self.missing:
                    path = model_aliases[path]
                custom = "/porory/" in path or path.endswith("fx_horadric_light_wp.particles") or path.endswith("t_fx_light_beam_angled_atlas_wp.texture")
                if custom or path in self.missing:
                    data = self.jcy_bytes(path)
                    origin = "jcy"
                    parsed = json.loads(data.decode("utf-8-sig")) if path.endswith(".json") else None
                else:
                    data = self.native_bytes(path)
                    origin = "native"
                    parsed = self.native_json(path) if path.endswith(".json") else None
                target = self.mpq / path
                # A required visual dependency must not remain a LiteHub empty override.
                if custom or not target.exists() or not target.stat().st_size or path.endswith((".particles", ".texture")):
                    self.put(path,data)
                if parsed is not None:
                    pending.update(references(parsed))
                else:
                    pending.update(references(data))
                audit.append({"path":path,"logical_path":logical_path,"origin":origin,"source_sha256":digest(data)})
                if len(visited)>5000:
                    raise ValueError("Dependency closure unexpectedly exceeds 5000 resources")
            print(f"Dependency closure: {len(visited)} checked, {len(pending-visited)} remaining",flush=True)
        self.dependency_audit = audit

    def validate(self):
        for path in self.modified:
            p = self.mpq / path
            if not p.is_file() or not p.stat().st_size:
                raise ValueError(f"Enhanced resource missing/empty: {path}")
        # Sound edits cannot modify unrelated states, skills or sound records.
        _, original = table(self.native_bytes("data/global/excel/states.txt"))
        _, actual = table((self.mpq/"data/global/excel/states.txt").read_bytes())
        assert len(original)==len(actual)
        for a,b in zip(original,actual):
            expected=copy.deepcopy(a)
            if a["state"] in BUFFS:expected["offsound"]="litehubplus_"+a["state"]+"_off"
            assert expected==b
        _, old_sounds=table(self.native_bytes("data/global/excel/sounds.txt"))
        _, new_sounds=table((self.mpq/"data/global/excel/sounds.txt").read_bytes())
        assert new_sounds[:len(old_sounds)]==old_sounds
        assert len({r['Sound'] for r in new_sounds if r['Sound'].startswith('litehubplus_')})==len(BUFFS)
        for name in ("bankexpansionlayouthd","playerinventoryexpansionlayouthd"):
            orig=self.native_json(LAYOUT+name+".json")
            actual=read_json(self.mpq/(LAYOUT+name+".json"))
            def remove_additions(d):
                if isinstance(d,dict):return {k:remove_additions(v) for k,v in d.items()}
                if isinstance(d,list):return [remove_additions(v) for v in d if not isinstance(v,dict) or not v.get('name','').startswith('LiteHubPlus')]
                return d
            assert remove_additions(actual)==orig
        self.features["verification"]={"tables_preserve_unrelated_rows":True,"native_ui_interactions_preserved":True,"runtime_verified":False}

    def build(self):
        report=read_json(self.base/"generation-manifest.json")
        if report.get("profile")!="main" or report.get("verified_output_integrity") is not True:
            raise ValueError("Expected a verified freshly generated main baseline")
        self.stage.mkdir(parents=True)
        source=self.base/(report["mod_name"]+".mpq")
        shutil.copytree(source,self.mpq)
        info=read_json(self.mpq/"modinfo.json")
        info["name"]="LiteHubPlus"
        write_json(self.mpq/"modinfo.json",info)
        write_json(self.stage/"baseline-generation-manifest.json",report)
        current=self.native_bytes("data/global/dataversionbuild.txt").decode('utf-8-sig').strip()
        if (self.mpq/"data/global/dataversionbuild.txt").read_text('utf-8-sig').strip()!=current:
            raise ValueError("Baseline does not match the installed game version")
        print("Adding map markers and route hints",flush=True); self.markers()
        print("Adding selected loot beams",flush=True); self.loot()
        print("Adding state-end audio",flush=True); self.buffs()
        print("Adding inventory/stash organization guides",flush=True); self.ui()
        print("Resolving required visual dependencies",flush=True); self.dependencies()
        self.validate()
        files={p.relative_to(self.mpq).as_posix():{"sha256":digest(p.read_bytes()),"bytes":p.stat().st_size} for p in sorted(self.mpq.rglob('*')) if p.is_file()}
        manifest={"producer":"d2r-litehub-plus-personal-builder","recipe_version":REVISION,"profile":"main","mod_name":"LiteHubPlus","game_data_version":current,"launch_arguments":"-mod LiteHubPlus -txt -assettestmode 1","runtime_verified":False,"verified_output_integrity":True,"features":self.features,"modified_paths":sorted(self.modified),"source_hashes":self.sources,"dependencies":self.dependency_audit,"files":files,"generated_bytes":sum(v['bytes'] for v in files.values())}
        write_json(self.stage/"enhancement-manifest.json",manifest)
        (self.stage/"README.txt").write_text("LiteHubPlus 个人验证版 r1\n启动参数：-mod LiteHubPlus -txt -assettestmode 1\n无需控制器。仅支持本次生成的国服数据版本 "+current+"。\n入口光圈、上下口标识；高塔及憎恨囚牢二层小站探索方向箭头。\nVex(26)至Zod(33)、三钥匙、三器官光柱。\nBO / BC / Shout 状态结束语音，复用本机 JCY 提示声音。\n背包6+4列参考分区；普通仓库四个5x5参考分区，保持原生页签和容量。\n本版未加入声纹或房间工具；可用 Hub 另行加工为新名称。\n文件与依赖检查通过，游戏内触发、方向、音量、分辨率尚待实测。\n箭头只提供探索方向，不定位未探索地图真实出口。\n分区仅为视觉参考，不会自动整理、锁定或扩容。\n重复施法/队友/死亡/切图时语音行为待实测；经典画面和手柄未验证。\n卸载：游戏关闭后移走 LiteHubPlus 目录，原 LiteHub 不受影响。\n\n素材来源：本机原版 CASC 与已安装 JCY（https://github.com/jcymeow/jcy）。\nJCY 整合的 Porory beacon 等素材保留路径归属；此成品供本机使用，未取得第三方素材再分发授权，未公开发布。\n详细文件摘要、来源、覆盖范围见 enhancement-manifest.json。\n",encoding='utf-8-sig')
        # Do not publish a stale stock generation manifest for an enhanced product.
        self.stage.rename(self.destination)
        print(json.dumps({"output":str(self.destination),"files":len(files),"bytes":manifest['generated_bytes'],"markers":len(self.features['map_markers']),"modified":len(self.modified)},ensure_ascii=False),flush=True)


if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("game","baseline","jcy","work","output","exporter"):
        parser.add_argument("--"+name,type=pathlib.Path,required=True)
    Builder(parser.parse_args()).build()
