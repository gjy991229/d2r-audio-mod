"""Prepare local branded LiteHub/BoHub/NullHub, without publishing update receipts."""
import argparse
import copy
import hashlib
import json
import pathlib
import shutil
import struct
from PIL import Image, ImageDraw

VERSION='2026.09.28.3'
URL='https://gitee.com/garyi7e/d2-rhub_-resource'
RIBBON='data/global/ui/layouts/mainmenubuttonribbonhd.json'
LINK_PANEL='data/global/ui/layouts/D2RHubReleaseLinkhd.json'
SPRITE='data/hd/global/ui/d2rhub/brand_button'


def read(p):return json.loads(p.read_text('utf-8-sig'))
def write(p,d):
    p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
def sha(b):return hashlib.sha256(b).hexdigest()


def sprite(image):
    frame_width=image.width//4
    header=bytearray(40);header[:4]=b'SpA1'
    struct.pack_into('<H',header,4,31);struct.pack_into('<H',header,6,frame_width)
    for offset,value in [(8,image.width),(12,image.height),(20,4),(32,image.width*image.height*4),(36,4)]:struct.pack_into('<I',header,offset,value)
    return bytes(header)+image.tobytes()


def graphics(logo,output):
    original=Image.open(logo).convert('RGBA')
    icon=original.copy();icon.thumbnail((144,144),Image.Resampling.LANCZOS)
    frames=[]
    for background,border in [((39,38,35,240),(118,97,59,230)),((29,28,25,245),(176,141,69,255)),((29,29,29,220),(72,69,62,210)),((56,50,37,245),(221,181,91,255))]:
        frame=Image.new('RGBA',(520,192),(0,0,0,0))
        ImageDraw.Draw(frame).rounded_rectangle((1,1,518,190),radius=12,fill=background,outline=border,width=2)
        frame.alpha_composite(icon,(25+(144-icon.width)//2,(192-icon.height)//2))
        frames.append(frame)
    atlas=Image.new('RGBA',(2080,192))
    for i,frame in enumerate(frames):atlas.alpha_composite(frame,(i*520,0))
    preview=frames[0].copy()
    # This is a UI composition preview. In-game text is rendered by native widgets.
    from PIL import ImageFont
    font=ImageFont.truetype('C:/Windows/Fonts/arial.ttf',40)
    ImageDraw.Draw(preview).text((195,70),'D2RHub',font=font,fill=(229,207,158,255))
    preview.save(output/'logo-button-preview.png')
    return {SPRITE+'.sprite':sprite(atlas),SPRITE+'.lowend.sprite':sprite(atlas.resize((1040,96),Image.Resampling.LANCZOS))}


def relay():
    # UKComplianceModal is the same URL-capable handler used by JCY's links.
    # Instantiated only following the user's Logo click. No launch-time navigation.
    return {'type':'UKComplianceModal','name':'D2RHubReleaseLink','fields':{'anchor':{'x':0.5,'y':0.5},'isDismissable':True},'children':[
        # TimerWidget keeps dispatching after expiry until its panel is removed.
        # Both timers expire on the first frame, in child order: dispatch once,
        # then immediately unload. A later unload allowed six browser launches.
        {'type':'TimerWidget','name':'OpenReleasePage','fields':{'time':0,'message':'ModalMessage:Confirm:'+URL}},
        {'type':'TimerWidget','name':'UnloadLinkPanel','fields':{'time':0,'message':'PanelManager:UnloadPanel:D2RHubReleaseLink'}}
    ]}


def branded_ribbon(original,name):
    d=copy.deepcopy(original)
    children=d.setdefault('children',[])
    if any(c.get('name','').startswith('D2RHubBrand') for c in children):raise ValueError('Already branded')
    children.extend([
        {'type':'ButtonWidget','name':'D2RHubBrandLogo','fields':{
            'rect':{'x':165,'y':510,'width':520,'height':192},
            'filename':'d2rhub/brand_button','hoveredFrame':3,'pressedFrame':1,'disabledFrame':2,
            'onClickMessage':'PanelManager:OpenPanel:D2RHubReleaseLink','tooltipString':'打开 D2RHub 发布与下载页面'
        },'children':[{'type':'TextBoxWidget','name':'BrandName','fields':{
            'rect':{'x':194,'y':58,'width':300,'height':80},'text':'D2RHub',
            'style':{'pointSize':48,'fontColor':{'r':229,'g':207,'b':158,'a':255},'alignment':{'h':'left','v':'center'}}
        }}]},
        {'type':'TextBoxWidget','name':'D2RHubBrandVersion','fields':{
            'rect':{'x':165,'y':716,'width':600,'height':55},'text':name+'  v'+VERSION,
            'style':{'pointSize':26,'fontColor':{'r':165,'g':156,'b':136,'a':255},'alignment':{'h':'left','v':'center'}}
        }},
        {'type':'TextBoxWidget','name':'D2RHubBrandThanks','fields':{
            'rect':{'x':165,'y':770,'width':600,'height':38},'text':'感谢 JCY',
            'style':{'pointSize':24,'fontColor':{'r':165,'g':156,'b':136,'a':255},'alignment':{'h':'left','v':'center'}}
        }}
    ])
    return d


def run(a):
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
    assets=graphics(a.logo,out)
    summaries=[]
    for name,source_name,profile in [('LiteHub','LiteHubPlusR2','main'),('BoHub','BoHub','filler'),('NullHub','NullHub','min')]:
        source=a.mods/source_name;source_mpq=source/(source_name+'.mpq')
        dest=out/name;mpq=dest/(name+'.mpq')
        if dest.exists():raise ValueError('Refusing to overwrite '+str(dest))
        before={p.relative_to(source_mpq).as_posix():sha(p.read_bytes()) for p in source_mpq.rglob('*') if p.is_file()}
        print('Preparing '+name,flush=True)
        shutil.copytree(source_mpq,mpq)
        info=read(mpq/'modinfo.json');info['name']=name;write(mpq/'modinfo.json',info)
        original=read(mpq/RIBBON);updated=branded_ribbon(original,name);write(mpq/RIBBON,updated)
        write(mpq/LINK_PANEL,relay())
        for rel,data in assets.items():
            p=mpq/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
        # The prior layout, including its Settings and Exit buttons, is unchanged.
        stripped=copy.deepcopy(updated);stripped['children']=[c for c in stripped['children'] if not c.get('name','').startswith('D2RHubBrand')]
        assert stripped==original
        allowed={RIBBON,LINK_PANEL,'modinfo.json',*assets.keys()}
        files={p.relative_to(mpq).as_posix():{'sha256':sha(p.read_bytes()),'bytes':p.stat().st_size} for p in sorted(mpq.rglob('*')) if p.is_file()}
        for rel,h in before.items():
            if rel not in allowed:assert files[rel]['sha256']==h,rel
        assert set(files)-set(before)=={LINK_PANEL,*assets.keys()}
        for rel,data in assets.items():
            w,h=struct.unpack_from('<II',data,8);assert len(data)==40+w*h*4
            assert struct.unpack_from('<I',data,20)[0]==4
        game_version=(mpq/'data/global/dataversionbuild.txt').read_text('utf-8-sig').strip()
        version={'mod_name':name,'mod_version':VERSION,'game_data_version':game_version,'release_url':URL,'channel':'local-preview','published':False}
        write(dest/'mod-version.json',version)
        if (source/'enhancement-manifest.json').exists():
            manifest=read(source/'enhancement-manifest.json')
        else:
            manifest={'producer':'d2rhub-local-mod-builder','profile':profile,'features':{}}
        manifest.update({'mod_name':name,'mod_version':VERSION,'game_data_version':game_version,'launch_arguments':f'-mod {name} -txt -assettestmode 1','files':files,'generated_bytes':sum(v['bytes'] for v in files.values()),'verified_output_integrity':True,'runtime_verified':False})
        manifest.setdefault('features',{})['main_menu_branding']={'logo_source_sha256':sha(a.logo.read_bytes()),'version':VERSION,'url':URL,'one_click_relay':True,'runtime_verified':False}
        manifest['modified_paths']=sorted(set(manifest.get('modified_paths',[]))|allowed)
        write(dest/'enhancement-manifest.json',manifest)
        # Original reports are evidence, not a false official install receipt.
        evidence=out/'source-records'/name;evidence.mkdir(parents=True)
        for p in source.glob('*.json'):shutil.copy2(p,evidence/p.name)
        (dest/'README.txt').write_text(f'{name} v{VERSION}\n启动：-mod {name} -txt -assettestmode 1\n主菜单新增 D2RHub LOGO，点击使用游戏原生 URL 处理面板打开：\n{URL}\nMod 版本与游戏数据版本（{game_version}）分开记录。\n本次为本机验证成品，未发布资源更新，不伪造官方安装记录。\n保留修改前游戏功能；LiteHub 沿用已确认的 Plus 布局、特殊地图箭头和重点掉落光柱，入口光圈与技能语音保持删除。\n本次只完成文件、按钮消息与依赖校验；LOGO 显示及浏览器跳转需游戏内验证。\n',encoding='utf-8-sig')
        summaries.append({'name':name,'source':source_name,'files':len(files),'version':VERSION,'changed_existing':sorted(k for k in before if files[k]['sha256']!=before[k]),'added':sorted(set(files)-set(before))})
    write(out/'validation-report.json',{'mods':summaries,'runtime_verified':False})
    print(json.dumps(summaries),flush=True)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('mods','output','logo'):p.add_argument('--'+name,type=pathlib.Path,required=True)
    run(p.parse_args())
