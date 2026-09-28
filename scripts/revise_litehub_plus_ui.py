"""Personal r2 correction: import actual installed JCY layouts with their UI dependencies."""
import argparse
import copy
import hashlib
import json
import pathlib
import re
import shutil

from build_litehub_plus import Builder, read_json, write_json, digest, LAYOUT


def strings(value):
    if isinstance(value,str):
        yield value
    elif isinstance(value,dict):
        for v in value.values():yield from strings(v)
    elif isinstance(value,list):
        for v in value:yield from strings(v)


def run(args):
    b=Builder(args)
    b.destination=args.output.resolve()/'LiteHubPlusR2'
    if b.destination.exists():raise ValueError('Refusing to replace existing r2')
    b.mpq=b.stage/'LiteHubPlusR2.mpq'
    source=args.source.resolve()
    manifest=read_json(source/'enhancement-manifest.json')
    if manifest['recipe_version']!=1:raise ValueError('Expected r1 source')
    b.stage.mkdir(parents=True)
    shutil.copytree(source/'LiteHubPlus.mpq',b.mpq)
    info=read_json(b.mpq/'modinfo.json');info['name']='LiteHubPlusR2';write_json(b.mpq/'modinfo.json',info)
    names=['playerinventoryexpansionlayouthd.json','bankexpansionlayouthd.json','JcyMiniCubehd.json','JcyMiniCubeClosehd.json']+[f'JcyStashPage{i}hd.json' for i in range(1,10)]
    docs={name:b.jcy_json(LAYOUT+name) for name in names}
    profile_path=LAYOUT+'_profilehd.json'
    profile=b.native_json(profile_path)
    jprofile=b.jcy_json(profile_path)
    # Pull only values referenced by the imported layouts, not JCY's lobby/HUD overrides.
    needed={s[1:] for d in docs.values() for s in strings(d) if s.startswith('$')}
    needed.update(['RightPanelRect','RightPanelAnchor','LeftPanelRect','LeftPanelAnchor'])
    done=set()
    while needed-done:
        for key in sorted(needed-done):
            done.add(key)
            if key in jprofile:profile[key]=copy.deepcopy(jprofile[key])
            if key not in profile:raise ValueError('Unknown UI variable '+key)
            needed.update(s[1:] for s in strings(profile[key]) if s.startswith('$'))

    # Use JCY's compact inventory and put its functional cube in the reserved left column.
    cube=docs['JcyMiniCubehd.json']
    cube['fields']['anchor']=copy.deepcopy(profile['RightPanelAnchor'])
    right=profile['RightPanelRect']
    cube['fields']['rect']={'x':right['x']+65,'y':right['y']+440}
    inv=docs['playerinventoryexpansionlayouthd.json']
    inv['children'].append({'type':'TextBoxWidget','name':'LiteHubPlusRevision','fields':{'rect':{'x':65,'y':350,'width':330,'height':65},'text':'LiteHubPlus r2','style':{'pointSize':26,'fontColor':{'r':205,'g':175,'b':105,'a':255},'alignment':{'h':'left','v':'center'}}}})
    bank=docs['bankexpansionlayouthd.json']
    # Layout import must not also auto-confirm unrelated modal dialogs.
    basic=next(c for c in bank['children'] if c['name']=='basicstash_container')
    basic['children']=[c for c in basic['children'] if c.get('name')!='autoconfirm']
    # Retain visible identity in the native title widget.
    next(c for c in bank['children'] if c['name']=='title').setdefault('fields',{})['text']='储藏箱 · LiteHubPlus r2'

    for name,doc in docs.items():b.put_json(LAYOUT+name,doc)
    b.put_json(profile_path,profile)

    # Discover all referenced UI sprite paths, including values supplied by styles.
    values=list(strings(list(docs.values())))+list(strings({k:profile[k] for k in done}))
    ui_refs=set()
    for s in values:
        normalized=s.replace('\\','/').lower()
        if '/' in normalized and not any(c in normalized for c in '\n\r:@$') and not normalized.startswith('data/'):
            if re.fullmatch(r'[a-z0-9_ /.-]+',normalized):ui_refs.add(normalized)
    candidates={f'data/hd/global/ui/{stem}{suffix}' for stem in ui_refs for suffix in ('.sprite','.lowend.sprite')}
    native_needed={p for p in candidates if not (b.jcy/p).is_file()}
    b.export(native_needed)
    copied=[]
    for p in sorted(candidates):
        if (b.jcy/p).is_file():data=b.jcy_bytes(p)
        elif p not in b.missing:data=b.native_bytes(p)
        else:continue
        if not data:raise ValueError('Required UI sprite is disabled: '+p)
        b.put(p,data);copied.append(p)
    missing_stems=[s for s in ui_refs if not any((b.mpq/f'data/hd/global/ui/{s}{suf}').is_file() for suf in ('.sprite','.lowend.sprite'))]
    if missing_stems:raise ValueError('Missing UI sprites: '+str(missing_stems))

    # Merge only localized keys actually referenced by these layouts and their texts.
    token=re.compile(r'@([A-Za-z0-9_]+)')
    wanted={key for s in values for key in token.findall(s)}
    local_tables={}
    lookup={}
    for p in (b.jcy/'data/local/lng/strings').glob('*.json'):
        try:rows=read_json(p)
        except (ValueError,UnicodeError):continue
        if not isinstance(rows,list):continue
        local_tables[p.name]=rows
        for row in rows:
            if isinstance(row,dict) and isinstance(row.get('Key'),str):lookup[row['Key']]=(p.name,row)
    selected={}
    checked=set()
    while wanted-checked:
        for key in sorted(wanted-checked):
            checked.add(key)
            if key not in lookup:continue # Native keys remain in the original game.
            filename,row=lookup[key]
            selected.setdefault(filename,{})[key]=row
            wanted.update(k for s in strings(row) for k in token.findall(s))
    b.export(['data/local/lng/strings/'+n for n in selected])
    max_id=2000000
    for filename,entries in selected.items():
        rel='data/local/lng/strings/'+filename
        rows=b.native_json(rel)
        bykey={r['Key']:i for i,r in enumerate(rows)}
        max_id=max([max_id]+[r.get('id',0) for r in rows if isinstance(r.get('id',0),int)])
        for key,row in entries.items():
            row=copy.deepcopy(row)
            if key in bykey:
                old=rows[bykey[key]]
                row['id']=old['id'];rows[bykey[key]]=row
            else:
                max_id+=1;row['id']=max_id;rows.append(row)
        b.put_json(rel,rows)

    # Mechanical invariants: UI changes never expand actual inventory/cube capacity.
    assert next(c for c in inv['children'] if c['name']=='grid')['fields']['rect']=={'x':92,'y':930}
    assert next(c for c in cube['children'] if c['name']=='grid')['fields']['cellCount']=={'x':3,'y':4}
    assert next(c for c in bank['children'] if c['name']=='BankTabs')['fields']['tabCount']==4
    # r2 does not claim to repair an unconfirmed sound-engine fault.
    for rel in ['data/global/excel/states.txt','data/global/excel/sounds.txt']:
        assert (b.mpq/rel).read_bytes()==(source/'LiteHubPlus.mpq'/rel).read_bytes()
    for name in names:
        for s in strings(docs[name]):
            for panel in re.findall(r'PanelManager:(?:OpenPanel|ClosePanel|UnloadPanel|TogglePanel):([A-Za-z0-9]+)',s):
                if panel.startswith('Jcy'):assert (b.mpq/(LAYOUT+panel+'hd.json')).is_file(),panel
    manifest['recipe_version']=2
    manifest['mod_name']='LiteHubPlusR2'
    manifest['launch_arguments']='-mod LiteHubPlusR2 -txt -assettestmode 1'
    manifest['runtime_verified']=False
    manifest['features']['inventory_layout']={'implementation':'JCY compact equipment layout, integrated mini cube and recipe tooltips','version_badge':'LiteHubPlus r2','capacity_changed':False}
    manifest['features']['stash_layout']={'implementation':'JCY four-tab layout with runeword reference and combined material page','capacity_changed':False}
    manifest['features']['verification']['native_ui_interactions_preserved']=False
    manifest['features']['verification']['jcy_ui_dependency_closure']=True
    manifest['features']['verification']['runtime_verified']=False
    manifest['corrections']=['r1 used native layouts with cosmetic grids, not JCY layouts','original LiteHub radial entrance beams remain inherited, not new work','audio runtime unconfirmed; saved account argument observed still points to LiteHub']
    manifest['modified_paths']=sorted(set(manifest['modified_paths'])|b.modified)
    manifest['source_hashes'].update(b.sources)
    manifest['ui_dependencies']={'sprites':copied,'layout_variables':sorted(done),'localized_keys':sorted(checked & lookup.keys())}
    manifest['files']={p.relative_to(b.mpq).as_posix():{'sha256':digest(p.read_bytes()),'bytes':p.stat().st_size} for p in sorted(b.mpq.rglob('*')) if p.is_file()}
    manifest['generated_bytes']=sum(v['bytes'] for v in manifest['files'].values())
    write_json(b.stage/'enhancement-manifest.json',manifest)
    (b.stage/'README.txt').write_text('LiteHubPlus r2：实际 JCY 布局修订版\n启动：-mod LiteHubPlusR2 -txt -assettestmode 1\n打开背包左栏应看到 LiteHubPlus r2；仓库标题也有 r2。未看到则不能确认加载此成品。\n背包：JCY 紧凑装备区、左栏迷你魔盒、公式提示。\n仓库：JCY 四页布局（个人、共享、符文之语、混合材料）；改变呈现，不改变容量。\n原版 LiteHub 放射光束仍保留，不能将其算作本次新增。新增高塔/憎恨囚牢探索箭头须进相应层测试。\n语音仍为 BO/BC/Shout 的状态结束提示，未宣称修复游戏引擎触发问题；先核实实际加载的 Mod 和 -txt。\n此版仅做文件、依赖、映射检查，游戏内效果未验证。\n素材来自本机 JCY https://github.com/jcymeow/jcy 与游戏原版，仅个人使用。\n',encoding='utf-8-sig')
    b.stage.rename(b.destination)
    print(json.dumps({'output':str(b.destination),'sprites':len(copied),'layouts':len(names),'localized_keys':len(checked & lookup.keys())}))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('game','baseline','jcy','work','output','exporter','source'):parser.add_argument('--'+name,type=pathlib.Path,required=True)
    run(parser.parse_args())
