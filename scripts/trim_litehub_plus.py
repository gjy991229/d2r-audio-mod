"""Remove only added entrance rings and buff voices from an existing Plus build."""
import argparse
import csv
import hashlib
import io
import json
import pathlib
import shutil


def read(p):return json.loads(p.read_text('utf-8-sig'))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def rows(p):return list(csv.DictReader(io.StringIO(p.read_text('utf-8-sig')),delimiter='\t'))


def run(a):
    source=a.source.resolve();target=a.output.resolve();baseline=a.baseline.resolve();native=a.native.resolve()
    if target.exists() or target==source or source in target.parents:raise ValueError('Fresh independent output required')
    m=read(source/'enhancement-manifest.json');name=m['mod_name'];old=source/(name+'.mpq')
    for rel,v in m['files'].items():
        if sha(old/rel)!=v['sha256']:raise ValueError('Source changed: '+rel)
    # Restore whole tables only after proving they contain exactly our voice edits.
    expected=rows(native/'data/global/excel/states.txt');actual=rows(old/'data/global/excel/states.txt')
    buffs=m['features']['buff_end_audio']['states']
    for r in expected:
        if r['state'] in buffs:r['offsound']='litehubplus_'+r['state']+'_off'
    assert expected==actual,'Unrelated state changes must be preserved instead'
    original_sounds=rows(native/'data/global/excel/sounds.txt');sounds=rows(old/'data/global/excel/sounds.txt')
    assert sounds[:len(original_sounds)]==original_sounds
    assert {r['Sound'] for r in sounds[len(original_sounds):]}=={'litehubplus_'+s+'_off' for s in buffs}
    shutil.copytree(source,target);mpq=target/(name+'.mpq');changed=set();removed=set()

    def restore(rel):
        p=mpq/rel;assert p.resolve().is_relative_to(mpq.resolve())
        if (baseline/rel).is_file():
            p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(baseline/rel,p)
        elif p.is_file():p.unlink();removed.add(rel)
        changed.add(rel)

    markers=m['features']['map_markers']
    for rel in markers:restore(rel)
    circle_deps=[d for d in m['dependencies'] if '/porory/beacon/' in d['path']]
    for d in circle_deps:
        for rel in d.get('resolved_paths',[d['path']]):restore(rel)
    for buff in buffs:restore(f'data/hd/global/sfx/litehubplus/{buff}_off.flac')
    for table in ('states','sounds'):
        for ext in ('txt','bin'):restore(f'data/global/excel/{table}.{ext}')
    # Every file outside the explicit removal scope must remain byte-identical.
    for rel,v in m['files'].items():
        if rel not in changed:assert sha(mpq/rel)==v['sha256'],rel
    for rel in markers:
        if (baseline/rel).exists():assert (mpq/rel).read_bytes()==(baseline/rel).read_bytes()
        else:assert not (mpq/rel).exists()
    for p in mpq.rglob('*.json'):
        if p.stat().st_size:
            raw=p.read_bytes()
            assert b'litehubplus_entrance_' not in raw,p
            assert b'data/hd/env/porory/beacon/' not in raw,p
    m['recipe_version']=3
    m['features']['map_markers']={'enabled':False,'behavior':'original LiteHub entrance resources restored'}
    m['features']['buff_end_audio']={'enabled':False,'behavior':'custom states/sounds overrides and voice files removed'}
    m['dependencies']=[d for d in m['dependencies'] if d not in circle_deps]
    m['modified_paths']=sorted(set(m['modified_paths'])-changed)
    m['changes_r3']={'restored_or_removed':sorted(changed),'deleted_files':sorted(removed),'all_other_files_byte_identical':True,'retained_ui_badge':'LiteHubPlus r2'}
    m['files']={p.relative_to(mpq).as_posix():{'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(mpq.rglob('*')) if p.is_file()}
    m['generated_bytes']=sum(v['bytes'] for v in m['files'].values())
    (target/'enhancement-manifest.json').write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    (target/'README.txt').write_text('LiteHubPlusR2 精简修订 r3\n启动参数：'+m['launch_arguments']+'\n已移除：新增入口光圈、BO/BC/Shout 状态结束语音。\n入口恢复原 LiteHub 效果。\n保留：高塔与憎恨囚牢探索箭头、JCY背包与仓库布局、迷你魔盒及公式提示、26–33号符文/钥匙/器官掉落光柱、现有主菜单快速开局。\n为保持已经确认正常的 UI 文件完全不变，界面仍显示 LiteHubPlus r2。精简版本见本说明与清单 recipe_version=3。\n仅验证文件与资源配置；未进行新的游戏内测试。\n',encoding='utf-8-sig')
    print(json.dumps({'output':str(target),'files':len(m['files']),'changed':len(changed),'removed':len(removed)}))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for n in ('source','output','baseline','native'):p.add_argument('--'+n,type=pathlib.Path,required=True)
    run(p.parse_args())
