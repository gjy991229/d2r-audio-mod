"""Prepare branded Mod fixes and configurable Act IV slots without changing other content."""
import argparse
import copy
import csv
import io
import json
import pathlib
import shutil
from brand_hub_mods import VERSION, URL, RIBBON, LINK_PANEL, read, write, sha, relay, branded_ribbon


def patch_actinfo(data, selected):
    text=data.decode('utf-8-sig')
    lines=text.splitlines(keepends=True);header=lines[0].rstrip('\r\n').split('\t')
    positions=[header.index(f'waypoint{i}') for i in range(4,10)];act=header.index('act')
    found=0;out=[]
    for line in lines:
        body=line.rstrip('\r\n');fields=body.split('\t')
        if fields[act]=='4':
            found+=1;assert len(fields)==len(header)
            for column,value in zip(positions,selected):fields[column]=value
            line='\t'.join(fields)+line[len(body):]
        out.append(line)
    assert found==1
    return ''.join(out).encode('utf-8')


def run(a):
    catalog=read(a.catalog);out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
    native=(a.native/'data/global/excel/actinfo.txt').read_bytes()
    for name in ['LiteHub','BoHub','NullHub']:
        src=a.mods/name;dest=out/name;mpq=dest/(name+'.mpq')
        if dest.exists():raise ValueError('Output exists: '+str(dest))
        m=read(src/'enhancement-manifest.json')
        for rel,entry in m['files'].items():assert sha((src/(name+'.mpq')/rel).read_bytes())==entry['sha256'],rel
        print('Preparing '+name,flush=True)
        shutil.copytree(src,dest)
        old=read(mpq/RIBBON)
        original=copy.deepcopy(old);original['children']=[c for c in original['children'] if not c.get('name','').startswith('D2RHubBrand')]
        updated=branded_ribbon(original,name)
        write(mpq/RIBBON,updated);write(mpq/LINK_PANEL,relay())
        assert len([c for c in updated['children'] if c.get('name')=='D2RHubBrandLogo'])==1
        timers=relay()['children'];assert len(timers)==2 and all(c['fields']['time']==0 for c in timers)
        assert timers[0]['fields']['message']=='ModalMessage:Confirm:'+URL
        assert timers[1]['fields']['message']=='PanelManager:UnloadPanel:D2RHubReleaseLink'
        changed={RIBBON,LINK_PANEL}
        if name in ['LiteHub','BoHub']:
            rel='data/global/excel/actinfo.txt';p=mpq/rel
            before=p.read_bytes() if p.exists() else native
            after=patch_actinfo(before,catalog['defaults']);p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(after)
            # Validate every unchanged act and the original three Act IV slots.
            arows=list(csv.DictReader(io.StringIO(before.decode('utf-8-sig')),delimiter='\t'))
            brows=list(csv.DictReader(io.StringIO(after.decode('utf-8-sig')),delimiter='\t'))
            for x,y in zip(arows,brows):
                for key in x:
                    if x['act']=='4' and key in [f'waypoint{i}' for i in range(4,10)]:continue
                    assert x[key]==y[key],key
            write(dest/'d2rhub-waypoints.json',{'revision':1,'feature':'act4_waypoints','catalog_game_data_version':catalog['game_data_version']})
            m['features']['act4_waypoints']={'selected':catalog['defaults'],'configurable_slots':[4,5,6,7,8,9],'original_three_preserved':True,'runtime_verified':False}
            changed.add(rel)
        m['mod_version']=VERSION
        m['features']['main_menu_branding'].update({'version':VERSION,'url':URL,'timer_dispatch':'open_then_unload_same_frame','attribution':'感谢 JCY','runtime_verified':False})
        m['modified_paths']=sorted(set(m['modified_paths'])|changed)
        for rel in changed:
            b=(mpq/rel).read_bytes();m['files'][rel]={'sha256':sha(b),'bytes':len(b)}
        for rel,entry in m['files'].items():assert sha((mpq/rel).read_bytes())==entry['sha256'],rel
        m['generated_bytes']=sum(v['bytes'] for v in m['files'].values())
        write(dest/'enhancement-manifest.json',m)
        version=read(dest/'mod-version.json');version.update({'mod_version':VERSION,'release_url':URL});write(dest/'mod-version.json',version)
        text=f'{name} v{VERSION}\n启动：-mod {name} -txt -assettestmode 1\nLOGO 链接：{URL}\nLOGO 下方显示“感谢 JCY”。打开链接与卸载中转面板改为同一帧，避免计时器连续触发。\n'
        if name!='NullHub':
            labels=[next(o['label_zh'] for o in catalog['options'] if o['id']==key) for key in catalog['defaults']]
            text+='第四幕第4–9项默认：'+'、'.join(labels)+'。\n使用带快捷传送配置的新 Hub，在 Mod 管理→设置中逐项调整。保存前关闭游戏；保存后重新启动生效。原有前三项不变。\n'
        text+='这是本机预览成品；未发布资源更新。文件校验通过，新的点击次数及传送行为仍待游戏内确认。\n'
        (dest/'README.txt').write_text(text,encoding='utf-8-sig')
        print(name+' verified; modified resources '+str(len(changed)),flush=True)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['mods','output','native','catalog']:p.add_argument('--'+n,type=pathlib.Path,required=True)
    run(p.parse_args())
