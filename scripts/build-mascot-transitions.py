"""Build native authored pose paths and time-specific backward fields.

All artwork is losslessly packed at its original 887px resolution. New poses
must be built-in imagegen outputs registered in the input JSON; this script
does not draw, upscale, inpaint, or fabricate character poses. RIFE is used only
for numerical correspondence/visibility, and is never shipped with the app.

Run: python scripts/build-mascot-transitions.py --spec <json> [--stem <name>]
The spec maps each stem and action row to additional source cells and optional
routes. Original idle and two endpoints remain byte-for-byte pixel-identical.
"""
from pathlib import Path
import argparse,hashlib,importlib.util,json,math,shutil
import cv2,numpy as np,torch
from PIL import Image

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('mascot_flow',ROOT/'scripts/build-mascot-flow.py')
flow=importlib.util.module_from_spec(spec);spec.loader.exec_module(flow)
rigid_spec=importlib.util.spec_from_file_location('mascot_rigid',ROOT/'scripts/build-mascot-rigid.py')
rigid=importlib.util.module_from_spec(rigid_spec);rigid_spec.loader.exec_module(rigid)
SIZE=896;BORDER=32;TILE=160;PAD=2;COLUMNS=16;SAMPLES=[0,.25,.5,.75,1]
CACHE=ROOT/'.scratch/mascot-overlap-fix/field-cache'

def tensor(image):
    rgba=np.array(image.resize((SIZE,SIZE),Image.Resampling.LANCZOS)).astype('float32')/255
    rgba[:,:,:3]*=rgba[:,:,3:]
    return torch.from_numpy(cv2.copyMakeBorder(rgba,BORDER,BORDER,BORDER,BORDER,cv2.BORDER_CONSTANT)).permute(2,0,1)[None].to(flow.DEVICE)

def encoded_fields(a,b,model,guides=None):
    prior=b'' if guides is None else (ROOT/'scripts/build-mascot-rigid.py').read_bytes()+json.dumps([{k:v.tolist()for k,v in guide.items()}for guide in guides],sort_keys=True).encode()
    digest=hashlib.sha256(f'rife4.25-path-v1-{SIZE}-{TILE}-{SAMPLES}'.encode()+np.array(a).tobytes()+np.array(b).tobytes()+prior).hexdigest()
    CACHE.mkdir(parents=True,exist_ok=True);cache=CACHE/f'{digest}.npz'
    if cache.exists():
        saved=np.load(cache);return saved['vectors'],saved['masks'],float(saved['scale'])
    ta,tb=tensor(a),tensor(b);fields=[];masks=[];identical=np.array_equal(np.array(a),np.array(b))
    for time in SAMPLES:
        if guides is not None:
            vectors=np.zeros((SIZE,SIZE,4),dtype='float32');mask=np.full((SIZE,SIZE),1-time,dtype='float32')
            vectors,mask=rigid.refine(a,b,vectors,mask,time,guides,model)
        elif identical:
            vectors=np.zeros((SIZE,SIZE,4),dtype='float32');mask=np.full((SIZE,SIZE),1-time,dtype='float32')
        else:
            with torch.inference_mode():predicted,logits,_=model(torch.cat([ta,tb],1),timestep=time,scale_list=[16,8,4,2,1])
            vectors=predicted[-1][0,:,BORDER:-BORDER,BORDER:-BORDER].permute(1,2,0).cpu().numpy();mask=logits.sigmoid()[0,0,BORDER:-BORDER,BORDER:-BORDER].cpu().numpy()
            if time==0:vectors[:,:,:2]=0;mask[:]=1
            if time==1:vectors[:,:,2:]=0;mask[:]=0
        # Apply endpoint invariants last, including after any local prior.
        if time==0:vectors[:,:,:2]=0;mask[:]=1
        if time==1:vectors[:,:,2:]=0;mask[:]=0
        fields.append(vectors);masks.append(mask)
    scale=max(1.,max(float(np.max(np.abs(v)))for v in fields))
    encoded=np.stack([np.clip(np.rint(cv2.resize(v,(TILE,TILE),interpolation=cv2.INTER_AREA)/scale*127+128),1,255).astype('uint8')for v in fields])
    visible=np.stack([np.clip(np.rint(cv2.resize(m,(TILE,TILE),interpolation=cv2.INTER_AREA)*255),0,255).astype('uint8')for m in masks])
    np.savez_compressed(cache,vectors=encoded,masks=visible,scale=scale)
    return encoded,visible,scale

def motion_arc(a,b,encoded,masks,scale):
    """Parameterize by measured premultiplied picture travel, not keyframe count.

    A large hand/body move must receive more time than two nearly identical
    poses. This changes speed along the same continuous path, never its images
    or the containing action duration. No acceptance threshold is used here.
    """
    side=160;grid=(np.arange(side,dtype='float32')+.5)/side;xx,yy=np.meshgrid(grid,grid)
    images=[]
    for image in [a,b]:
        rgba=np.array(image).astype('float32')/255;rgba[:,:,:3]=np.rint(rgba[:,:,:3]*rgba[:,:,3:]*255)/255;images.append(rgba)
    pictures=[]
    for vectors,mask in zip(encoded,masks):
        field=cv2.remap(vectors.astype('float32')-128,xx*(TILE-1),yy*(TILE-1),cv2.INTER_LINEAR)*scale/SIZE/127
        visible=cv2.remap(mask.astype('float32')/255,xx*(TILE-1),yy*(TILE-1),cv2.INTER_LINEAR)[:,:,None]
        warped=[]
        for i,rgba in enumerate(images):
            height,width=rgba.shape[:2];px=xx+field[:,:,i*2];py=yy+field[:,:,i*2+1]
            frame=cv2.remap(rgba,np.clip(px*width-.5,0,width-1),np.clip(py*height-.5,0,height-1),cv2.INTER_LINEAR)
            frame[(px<0)|(px>1)|(py<0)|(py>1)]=0;warped.append(frame)
        pictures.append(warped[0]*visible+warped[1]*(1-visible))
    distances=np.array([max(.0001,float(np.mean(np.abs(right-left))*255))for left,right in zip(pictures,pictures[1:])])
    cumulative=np.r_[0,np.cumsum(distances)];return (cumulative/cumulative[-1]).tolist(),float(cumulative[-1])

def build(stem,config,model):
    target=ROOT/'src/assets/mascots-hd'/stem
    output=ROOT/'.scratch/mascot-overlap-fix/staged'/stem;output.mkdir(parents=True,exist_ok=True)
    original=json.loads((target/'manifest.json').read_text(encoding='utf-8'))
    idle=Image.open(target/'idle.webp').convert('RGBA');n=idle.width
    if n<768:raise ValueError('Native HD source required')
    actions=[];layouts=[];routes={};edges={};sources=[]
    for row in range(8):
        im=Image.open(target/f'action-{row}.webp').convert('RGBA')
        if im.size!=(n*2,n):raise ValueError(f'Invalid original action {stem}/{row}')
        frames=[idle,idle if row==0 else im.crop((0,0,n,n)),im.crop((n,0,n*2,n))]
        entry=config.get(str(row),{})
        for extra in entry.get('sources',[]):
            path=Path(extra['path']);path=path if path.is_absolute()else ROOT/path
            source=Image.open(path).convert('RGBA');cols=extra.get('columns',2);rows=extra.get('rows',1)
            if not np.any(np.array(source)[:,:,3]==0):raise ValueError(f'Native sprite requires a genuinely transparent background: {path}')
            original_size=source.size;pad_right=extra.get('transparentPaddingRight',0)
            if pad_right:
                if pad_right!=1 or source.width+pad_right!=cols*n or source.height!=rows*n or np.array(source)[:,-1,3].max()!=0:raise ValueError(f'Invalid explicit transparent edge padding: {path}')
                padded=Image.new('RGBA',(cols*n,rows*n));padded.paste(source,(0,0));source=padded
            if source.width!=cols*n or source.height!=rows*n:raise ValueError(f'No upscaling allowed: {path} {source.size}, expected {cols*n}x{rows*n}')
            for cell in extra.get('cells',list(range(cols*rows))):
                x,y=cell%cols*n,cell//cols*n;frames.append(source.crop((x,y,x+n,y+n)))
            sources.append({'row':row,'path':str(path.relative_to(ROOT)),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'framePixels':n,'originalDimensions':list(original_size),'transparentPaddingRight':pad_right})
        count=len(frames)-1;columns=min(3,count) if count>4 else 2;rows=math.ceil(count/columns)
        if max(columns*n,rows*n)>4096:raise ValueError(f'Action atlas exceeds portable 4096px limit: {stem}/{row}')
        packed=Image.new('RGBA',(columns*n,rows*n))
        for index,frame in enumerate(frames[1:]):packed.paste(frame,((index%columns)*n,(index//columns)*n))
        packed.save(output/f'motion-{row}.webp',lossless=True,exact=True,method=6)
        exported=Image.open(output/f'motion-{row}.webp').convert('RGBA')
        if not np.array_equal(np.array(exported),np.array(packed)):raise ValueError('Lossless packing changed pixels')
        layouts.append({'columns':columns,'rows':rows,'count':count,'framePixels':n});actions.append(frames)
        first=entry.get('toFirst',[0,1]);second=entry.get('firstToSecond',[1,2])
        if first[0]!=0 or first[-1]!=1 or second[0]!=1 or second[-1]!=2:raise ValueError(f'Invalid authored route {stem}/{row}')
        combined=first+second[1:]
        row_routes={(0,1):first,(1,2):second,(0,2):entry.get('toSecond',combined),(0,3):[0,0],(1,3):first[::-1],(2,3):entry.get('toSecond',combined)[::-1]}
        for (a,b),route in row_routes.items():
            if any(not isinstance(i,int)or i<0 or i>=len(frames)for i in route):raise ValueError('Out-of-range authored pose')
            routes[f'{row}:{a}:{b}']=route
            for x,y in zip(route,route[1:]):
                if x!=y:edges[(row,min(x,y),max(x,y))]=None
    guide_data={}
    for row,frames in enumerate(actions):
        for kind,field,derive in [('card','rigidSignBounds',rigid.guide),('cutout','handBounds',rigid.cutout_guide)]:
            for index,bounds in config.get(str(row),{}).get(field,{}).items():guide_data[(row,int(index))]=(kind,derive(frames[int(index)],bounds,SIZE))
    guided_count=sum((row,a)in guide_data and (row,b)in guide_data for row,a,b in edges)
    cutouts={key:value for key,value in guide_data.items()if 'mask'in value[1]}
    stride=TILE+PAD*2;total=(len(edges)+guided_count)*len(SAMPLES)+len(cutouts);height=math.ceil(total/COLUMNS)*stride;width=COLUMNS*stride
    if max(width,height)>4096:raise ValueError('Motion maps exceed portable 4096px allocation')
    vectors_png=np.zeros((height,width,4),dtype='uint8');visibility_png=np.zeros((height,width),dtype='uint8');pairs={};rigid_guides={};cursor=0;segment_stops={};segment_distances={}
    for edge_index,(row,a,b)in enumerate(edges):
        guides=[guide_data[(row,i)][1]for i in [a,b]]if(row,a)in guide_data and(row,b)in guide_data else None
        sets=[encoded_fields(actions[row][a],actions[row][b],model)]
        segment_stops[f'{row}:{a}:{b}'],segment_distances[f'{row}:{a}:{b}']=motion_arc(actions[row][a],actions[row][b],*sets[0])
        if guides is not None:
            sets.append(encoded_fields(actions[row][a],actions[row][b],model,guides))
            for i,g in zip([a,b],guides):rigid_guides[f'{row}:{i}']={'word':(g['word']/SIZE).tolist(),'bounds':g['bounds'].tolist(),'kind':guide_data[(row,i)][0]}
        pairs[f'{row}:{a}:{b}']=[]
        for fields,masks,scale in sets:
            pairs[f'{row}:{a}:{b}'].extend([cursor,round(scale/SIZE/127,12)])
            for offset,(encoded,visible)in enumerate(zip(fields,masks)):
                index=cursor+offset;r,c=divmod(index,COLUMNS)
                vectors_png[r*stride:(r+1)*stride,c*stride:(c+1)*stride]=cv2.copyMakeBorder(encoded,PAD,PAD,PAD,PAD,cv2.BORDER_REPLICATE)
                visibility_png[r*stride:(r+1)*stride,c*stride:(c+1)*stride]=cv2.copyMakeBorder(visible,PAD,PAD,PAD,PAD,cv2.BORDER_REPLICATE)
            cursor+=len(SAMPLES)
        print(f'{stem} {edge_index+1}/{len(edges)}: {row}/{a}->{b}',flush=True)
    for (row,index),(_,g)in cutouts.items():
        corners=(g['outer']*n/SIZE).astype('float32');destination=np.array([[0,0],[TILE-1,0],[TILE-1,TILE-1],[0,TILE-1]],dtype='float32')
        matte=np.clip(np.rint(cv2.warpPerspective(g['mask'],cv2.getPerspectiveTransform(corners,destination),(TILE,TILE),flags=cv2.INTER_LINEAR)*255),0,255).astype('uint8')
        r,c=divmod(cursor,COLUMNS);visibility_png[r*stride:(r+1)*stride,c*stride:(c+1)*stride]=cv2.copyMakeBorder(matte,PAD,PAD,PAD,PAD,cv2.BORDER_REPLICATE)
        rigid_guides[f'{row}:{index}']['maskTile']=cursor;cursor+=1
    cv2.imwrite(str(output/'motion-flow.png'),cv2.cvtColor(vectors_png,cv2.COLOR_RGBA2BGRA),[cv2.IMWRITE_PNG_COMPRESSION,9])
    cv2.imwrite(str(output/'motion-visibility.png'),visibility_png,[cv2.IMWRITE_PNG_COMPRESSION,9])
    route_stops={}
    for key,route in routes.items():
        row=key.split(':')[0]
        distances=[segment_distances.get(f'{row}:{min(a,b)}:{max(a,b)}',.0004)for a,b in zip(route,route[1:])]
        cumulative=np.r_[0,np.cumsum(distances)];route_stops[key]=(cumulative/cumulative[-1]).tolist()
    manifest={'algorithm':'rife4.25-keyframe-paths-v1','layout':'authored-action-paths','framePixels':n,'tile':TILE,'padding':PAD,'columns':COLUMNS,'width':width,'height':height,'pairs':pairs,
              'transitions':{'routes':routes,'layouts':layouts,'samples':SAMPLES,'routeStops':route_stops,'segmentStops':segment_stops},'rigidGuides':rigid_guides,'visibilityChannels':1,'originalSources':original.get('sources',[]),'additionalSources':sources}
    (output/'motion-manifest.json').write_text(json.dumps(manifest,separators=(',',':'))+'\n',encoding='utf-8')
    # Publish only after every image, route and numerical field has succeeded.
    # The manifest is last; callers should verify after this command completes.
    for name in [*[f'motion-{row}.webp'for row in range(8)],'motion-flow.png','motion-visibility.png','motion-manifest.json']:
        shutil.copyfile(output/name,target/name)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--spec',required=True);parser.add_argument('--stem',action='append');args=parser.parse_args()
    cv2.setNumThreads(2);torch.set_num_threads(8);config=json.loads(Path(args.spec).read_text(encoding='utf-8'));model=flow.load_model()
    for stem in args.stem or config:build(stem,config[stem],model)
