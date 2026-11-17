"""Numerical rigid-prop correspondence; all rendered colors remain source pixels."""
import cv2
import numpy as np
import torch

def quad_order(points):
    points=np.asarray(points,dtype='float32');s=points.sum(1);d=points[:,1]-points[:,0]
    return points[[s.argmin(),d.argmin(),s.argmax(),d.argmax()]]

def guide(image,bounds,size):
    pixels=np.array(image.convert('RGBA'));h,w=pixels.shape[:2]
    x0,y0,x1,y1=bounds
    color=pixels[:,:,:3].astype('float32');maximum=color.max(-1);minimum=color.min(-1)
    white=(pixels[:,:,3]>220)&(maximum>210)&(minimum>185)&(minimum>maximum*.80)
    region=np.zeros((h,w),dtype='uint8');region[y0:y1,x0:x1]=1;white=(white&region.astype(bool)).astype('uint8')
    count,labels,stats,_=cv2.connectedComponentsWithStats(white,8)
    selected=np.zeros_like(white)
    for i in range(1,count):
        if 20<=stats[i,cv2.CC_STAT_AREA] and stats[i,cv2.CC_STAT_HEIGHT]>=12 and stats[i,cv2.CC_STAT_WIDTH]>=8:selected[labels==i]=1
    yy,xx=np.where(selected)
    if len(xx)<150:raise ValueError(f'Rigid sign text could not be identified inside {bounds}')
    word=quad_order(cv2.boxPoints(cv2.minAreaRect(np.c_[xx,yy].astype('float32'))))
    # Locate the opaque card's border in its own text coordinates. Inferring a
    # large box from glyph height also captures the adjacent microphone/face.
    canonical=np.array([[0,0],[1,0],[1,1],[0,1]],dtype='float32')
    homography=cv2.getPerspectiveTransform(canonical,word)
    gx=np.linspace(-.5,1.5,256);gy=np.linspace(-1,2,256);xx,yy=np.meshgrid(gx,gy)
    sample=project(homography,np.stack([xx,yy],-1))
    opaque=cv2.remap(pixels[:,:,3].astype('float32')/255,sample[:,:,0],sample[:,:,1],cv2.INTER_LINEAR,borderMode=cv2.BORDER_CONSTANT)>.9
    cx=int(np.argmin(abs(gx-.5)));cy=int(np.argmin(abs(gy-.5)))
    right=[];left=[];bottom=[];top=[]
    for y in np.where((gy>.2)&(gy<.8))[0]:
        empty=np.where(~opaque[y,cx:])[0]
        if len(empty):right.append(gx[cx+empty[0]])
        empty=np.where(~opaque[y,:cx+1])[0]
        if len(empty):left.append(gx[empty[-1]])
    for x in np.where((gx>.2)&(gx<.8))[0]:
        empty=np.where(~opaque[cy:,x])[0]
        if len(empty):bottom.append(gy[cy+empty[0]])
        empty=np.where(~opaque[:cy+1,x])[0]
        if len(empty):top.append(gy[empty[-1]])
    r=float(np.median([v for v in right if 1.02<v<1.5]or[1.15]));l=float(np.median([v for v in left if -.45<v<-.02]or[1-r]))
    bot=float(np.median([v for v in bottom if 1.1<v<1.95]or[1.55]));t=1-bot
    # Only the card is constrained. The already-correct hand/arm and microphone
    # retain their independent body flow and visibility.
    outer=project(homography,np.array([[l,t],[r,t],[r,bot],[l,bot]],dtype='float32'))
    matte=np.zeros((h,w),dtype='float32');cv2.fillConvexPoly(matte,np.rint(outer).astype('int32'),1)
    # The purple CLEAR plate never contains the mint headset/microphone. They
    # remain in the body layer even where a rounded plate corner crosses them.
    red,green,blue=color.transpose(2,0,1)
    mint=(green>red*1.18)&(green>blue*.90)
    matte*=~mint;matte*=pixels[:,:,3]/255
    return {'word':word*size/w,'outer':outer*size/w,'bounds':np.array([l,t,r,bot],dtype='float32'),'mask':matte}

def cutout_guide(image,bounds,size):
    pixels=np.array(image.convert('RGBA'));h,w=pixels.shape[:2];rgb=pixels[:,:,:3].astype('float32')
    r,g,b=rgb.transpose(2,0,1);x0,y0,x1,y1=bounds
    area=np.zeros((h,w),dtype='uint8');area[y0:y1,x0:x1]=1
    skin=((r>150)&(r>g*1.45)&(r>b*2)&(pixels[:,:,3]>220)&area.astype(bool)).astype('uint8')
    count,labels,stats,_=cv2.connectedComponentsWithStats(skin,8)
    if count<2:raise ValueError('Hand correspondence requires a visible skin component')
    selected=1+int(np.argmax(stats[1:,cv2.CC_STAT_AREA]));mask=(labels==selected).astype('float32')
    # A one-pixel soft matte retains antialiased skin edges; this is a motion
    # representation, never painted replacement artwork.
    mask=cv2.GaussianBlur(cv2.dilate(mask,np.ones((3,3),dtype='uint8')),(3,3),.5)
    yy,xx=np.where(mask>.01);left=max(0,int(xx.min())-8);right=min(w,int(xx.max())+9);top=max(0,int(yy.min())-8);bottom=min(h,int(yy.max())+9)
    word=np.array([[left,top],[right,top],[right,bottom],[left,bottom]],dtype='float32')*size/w
    return {'word':word,'outer':word.copy(),'bounds':np.array([0,0,1,1],dtype='float32'),'mask':mask}

def project(matrix,points):
    shape=points.shape;flat=points.reshape(-1,2)
    result=np.c_[flat,np.ones(len(flat))]@matrix.T
    return (result[:,:2]/result[:,2:]).reshape(shape).astype('float32')

def refine(a,b,field,visibility,time,guides,model):
    size=field.shape[0];ga,gb=guides
    target=ga['word']*(1-time)+gb['word']*time
    ha=cv2.getPerspectiveTransform(ga['word'].astype('float32'),target.astype('float32'))
    hb=cv2.getPerspectiveTransform(gb['word'].astype('float32'),target.astype('float32'))
    images=[];masks=[]
    for image,g,hmat in zip([a,b],[ga,gb],[ha,hb]):
        rgba=np.array(image.resize((size,size))).astype('float32')/255;rgba[:,:,:3]*=rgba[:,:,3:]
        if 'mask'in g:mask=cv2.resize(g['mask'],(size,size),interpolation=cv2.INTER_LINEAR)
        else:
            mask=np.zeros((size,size),dtype='float32');cv2.fillConvexPoly(mask,np.rint(g['outer']).astype('int32'),1)
        masks.append(mask)
        images.append(cv2.warpPerspective(rgba*mask[:,:,None],hmat,(size,size),flags=cv2.INTER_LINEAR,borderMode=cv2.BORDER_CONSTANT))
    expected=ga['outer']*(1-time)+gb['outer']*time
    x0=max(0,int(expected[:,0].min())-24);x1=min(size,int(expected[:,0].max())+25)
    y0=max(0,int(expected[:,1].min())-24);y1=min(size,int(expected[:,1].max())+25)
    rh,rw=y1-y0,x1-x0;ph=(-rh)%64;pw=(-rw)%64
    residual=np.zeros_like(field);local_mask=np.full((size,size),1-time,dtype='float32')
    if 0<time<1:
        device=next(model.parameters()).device
        inputs=[torch.from_numpy(cv2.copyMakeBorder(im[y0:y1,x0:x1],0,ph,0,pw,cv2.BORDER_CONSTANT)).permute(2,0,1)[None].to(device)for im in images]
        with torch.inference_mode():flows,logits,_=model(torch.cat(inputs,1),timestep=time,scale_list=[16,8,4,2,1])
        residual[y0:y1,x0:x1]=flows[-1][0,:,:rh,:rw].permute(1,2,0).cpu().numpy()
        local_mask[y0:y1,x0:x1]=logits.sigmoid()[0,0,:rh,:rw].cpu().numpy()
    yy,xx=np.mgrid[:size,:size];points=np.stack([xx,yy],-1).astype('float32')
    refined=np.concatenate([project(np.linalg.inv(ha),points+residual[:,:,:2])-points,project(np.linalg.inv(hb),points+residual[:,:,2:])-points],-1)
    # This is a separate prop layer, not a discontinuous overwrite of the
    # body's displacement field. The renderer clips it to its actual quad.
    field=refined;visibility=local_mask
    if time==0:field[:,:,:2]=0;visibility[:]=1
    if time==1:field[:,:,2:]=0;visibility[:]=0
    return field,visibility
